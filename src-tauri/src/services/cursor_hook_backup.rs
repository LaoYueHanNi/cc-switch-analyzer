//! Cursor Hook `requests.jsonl` 备份：只读复制到应用目录，不改 Cursor 源文件。

use std::fs;
use std::path::Path;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::Serialize;

use crate::services::cursor_hook_merge::{self, HookMergeResult};
use crate::services::cursor_local_hook::{
    read_setting_raw, requests_jsonl_path, write_setting_raw,
};
use crate::utils;

pub const HOOK_BACKUP_PERIOD_KEY: &str = "cursor_hook_backup_period";
pub const MAX_HOOK_BACKUPS: usize = 50;
const BACKUP_PREFIX: &str = "requests-";
const BACKUP_SUFFIX: &str = ".jsonl";

/// 备份周期：关 / 每天（默认每天）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookBackupPeriod {
    Off,
    Daily,
}

impl HookBackupPeriod {
    pub fn as_str(self) -> &'static str {
        match self {
            HookBackupPeriod::Off => "off",
            HookBackupPeriod::Daily => "daily",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "off" | "0" | "false" | "disabled" => HookBackupPeriod::Off,
            _ => HookBackupPeriod::Daily,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookBackupInfo {
    pub period: String,
    pub backup_count: i64,
    pub last_backup_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookBackupResult {
    pub backed_up: bool,
    pub path: Option<String>,
    pub skipped_reason: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge: Option<HookMergeResult>,
}

pub fn get_hook_backup_period() -> HookBackupPeriod {
    read_setting_raw(HOOK_BACKUP_PERIOD_KEY)
        .map(|v| HookBackupPeriod::parse(&v))
        .unwrap_or(HookBackupPeriod::Daily)
}

pub fn set_hook_backup_period(period: &str) -> Result<HookBackupPeriod, String> {
    let parsed = HookBackupPeriod::parse(period);
    write_setting_raw(HOOK_BACKUP_PERIOD_KEY, parsed.as_str())?;
    Ok(parsed)
}

pub fn backup_status() -> HookBackupInfo {
    let dir = utils::get_hook_backup_dir().ok();
    let (backup_count, last_backup_at) = dir
        .as_ref()
        .map(|d| list_backup_stats(d))
        .unwrap_or((0, None));
    HookBackupInfo {
        period: get_hook_backup_period().as_str().to_string(),
        backup_count,
        last_backup_at,
    }
}

/// 同步 CSV 成功后顺带触发：周期关 / 源不存在 / 今日已备 → 静默跳过；成功备份后归整源日志。
pub fn maybe_backup_after_sync() {
    if get_hook_backup_period() == HookBackupPeriod::Off {
        log::debug!("[CURSOR] hook backup skip after sync: period=off");
        return;
    }
    match backup_with_policy(false, true) {
        Ok(r) => {
            if r.backed_up {
                log::info!("[CURSOR] hook backup after sync: {}", r.message);
            } else {
                log::debug!(
                    "[CURSOR] hook backup after sync skipped: {}",
                    r.skipped_reason.as_deref().unwrap_or("unknown")
                );
            }
        }
        Err(e) => log::warn!("[CURSOR] hook backup after sync failed: {}", e),
    }
}

/// 立即备份：不受「每天一次」限制；**不**归整源文件。源不存在返回明确错误。
pub fn backup_now() -> Result<HookBackupResult, String> {
    backup_with_policy(true, false)
}

/// 仅归整本机 `requests.jsonl`（不备份）。
pub fn merge_hooks_now() -> Result<HookMergeResult, String> {
    let source = requests_jsonl_path()?;
    if !source.exists() {
        return Err("本机 Hook 日志不存在（requests.jsonl），无法归整".to_string());
    }
    cursor_hook_merge::merge_requests_jsonl(&source)
}

fn backup_with_policy(force: bool, merge_after: bool) -> Result<HookBackupResult, String> {
    let source = requests_jsonl_path()?;
    let backup_dir = utils::get_hook_backup_dir()?;
    let now = Local::now();
    backup_at_paths(&source, &backup_dir, now, force, MAX_HOOK_BACKUPS, merge_after)
}

/// 可测核心：只读复制 source → backup_dir/requests-YYYYMMDD-HHMMSS.jsonl，并裁剪份数。
/// `merge_after` 为 true 时，备份成功后归整源文件。
pub fn backup_at_paths(
    source: &Path,
    backup_dir: &Path,
    now: DateTime<Local>,
    force: bool,
    max_keep: usize,
    merge_after: bool,
) -> Result<HookBackupResult, String> {
    if !source.exists() {
        if force {
            return Err("本机 Hook 日志不存在（requests.jsonl），无法备份".to_string());
        }
        return Ok(HookBackupResult {
            backed_up: false,
            path: None,
            skipped_reason: Some("source_missing".into()),
            message: "源文件不存在，已跳过".into(),
            merge: None,
        });
    }

    let day = now.format("%Y%m%d").to_string();
    if !force && has_backup_for_day(backup_dir, &day) {
        return Ok(HookBackupResult {
            backed_up: false,
            path: None,
            skipped_reason: Some("already_today".into()),
            message: "今日已备份，已跳过".into(),
            merge: None,
        });
    }

    fs::create_dir_all(backup_dir).map_err(|e| format!("创建备份目录失败: {}", e))?;
    let dest = backup_dir.join(format!(
        "{}{}{}",
        BACKUP_PREFIX,
        now.format("%Y%m%d-%H%M%S"),
        BACKUP_SUFFIX
    ));

    fs::copy(source, &dest).map_err(|e| format!("复制 Hook 日志失败: {}", e))?;
    prune_backups(backup_dir, max_keep)?;

    let merge = if merge_after {
        match cursor_hook_merge::merge_requests_jsonl(source) {
            Ok(m) => {
                if m.merged {
                    log::info!("[CURSOR] hook merge: {}", m.message);
                } else {
                    log::debug!("[CURSOR] hook merge: {}", m.message);
                }
                Some(m)
            }
            Err(e) => {
                log::warn!("[CURSOR] hook merge failed: {}", e);
                return Err(format!("备份成功但归整失败: {}", e));
            }
        }
    } else {
        None
    };

    let backup_msg = format!(
        "已备份到 {}",
        dest.file_name().unwrap_or_default().to_string_lossy()
    );
    let message = merge
        .as_ref()
        .map(|m| format!("{}；{}", backup_msg, m.message))
        .unwrap_or(backup_msg);

    Ok(HookBackupResult {
        backed_up: true,
        path: Some(dest.to_string_lossy().to_string()),
        skipped_reason: None,
        message,
        merge,
    })
}

fn parse_backup_stem(name: &str) -> Option<NaiveDateTime> {
    let rest = name.strip_prefix(BACKUP_PREFIX)?.strip_suffix(BACKUP_SUFFIX)?;
    NaiveDateTime::parse_from_str(rest, "%Y%m%d-%H%M%S").ok()
}

fn is_backup_filename(name: &str) -> bool {
    parse_backup_stem(name).is_some()
}

fn has_backup_for_day(backup_dir: &Path, yyyymmdd: &str) -> bool {
    let Ok(entries) = fs::read_dir(backup_dir) else {
        return false;
    };
    let prefix = format!("{}{}", BACKUP_PREFIX, yyyymmdd);
    entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .any(|name| name.starts_with(&prefix) && is_backup_filename(&name))
}

fn list_backup_names(backup_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(backup_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_backup_filename(n))
        .collect();
    names.sort();
    names
}

fn list_backup_stats(backup_dir: &Path) -> (i64, Option<i64>) {
    let names = list_backup_names(backup_dir);
    let count = names.len() as i64;
    let last = names.last().and_then(|n| {
        let ndt = parse_backup_stem(n)?;
        Local
            .from_local_datetime(&ndt)
            .single()
            .map(|dt| dt.timestamp())
    });
    (count, last)
}

/// 保留最近 `max_keep` 份，删最旧的。
pub fn prune_backups(backup_dir: &Path, max_keep: usize) -> Result<usize, String> {
    if max_keep == 0 {
        return Ok(0);
    }
    let names = list_backup_names(backup_dir);
    if names.len() <= max_keep {
        return Ok(0);
    }
    let remove_count = names.len() - max_keep;
    let mut removed = 0usize;
    for name in names.into_iter().take(remove_count) {
        let path = backup_dir.join(&name);
        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_src(dir: &Path, content: &str) -> PathBuf {
        let p = dir.join("requests.jsonl");
        fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn test_parse_period() {
        assert_eq!(HookBackupPeriod::parse("off"), HookBackupPeriod::Off);
        assert_eq!(HookBackupPeriod::parse("daily"), HookBackupPeriod::Daily);
        assert_eq!(HookBackupPeriod::parse(""), HookBackupPeriod::Daily);
        assert_eq!(HookBackupPeriod::parse("weird"), HookBackupPeriod::Daily);
    }

    #[test]
    fn test_backup_creates_timestamped_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let bak_dir = tmp.path().join("bak");
        fs::create_dir_all(&src_dir).unwrap();
        let src = write_src(&src_dir, "{\"model\":\"grok\"}\n");

        let now = Local
            .with_ymd_and_hms(2026, 7, 15, 14, 30, 45)
            .single()
            .unwrap();
        let r = backup_at_paths(&src, &bak_dir, now, true, 50, false).unwrap();
        assert!(r.backed_up);
        let dest = bak_dir.join("requests-20260715-143045.jsonl");
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "{\"model\":\"grok\"}\n");
        // 立即备份不归整，源文件保持原样
        assert_eq!(fs::read_to_string(&src).unwrap(), "{\"model\":\"grok\"}\n");
        assert!(r.merge.is_none());
    }

    #[test]
    fn test_auto_backup_merges_source() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let bak_dir = tmp.path().join("bak");
        fs::create_dir_all(&src_dir).unwrap();
        // 同模型短窗内可折叠事件，归整后应变少
        let src = write_src(
            &src_dir,
            concat!(
                r#"{"ts_utc":"2026-07-15T06:00:00.000Z","hook_event_name":"preToolUse","model":"grok"}"#,
                "\n",
                r#"{"ts_utc":"2026-07-15T06:00:02.000Z","hook_event_name":"preToolUse","model":"grok"}"#,
                "\n",
            ),
        );
        let now = Local
            .with_ymd_and_hms(2026, 7, 15, 14, 30, 45)
            .single()
            .unwrap();
        let r = backup_at_paths(&src, &bak_dir, now, false, 50, true).unwrap();
        assert!(r.backed_up);
        assert!(r.merge.is_some());
        // 备份副本是归整前的全文
        let bak = fs::read_to_string(bak_dir.join("requests-20260715-143045.jsonl")).unwrap();
        assert_eq!(bak.lines().count(), 2);
    }

    #[test]
    fn test_daily_skip_same_day_unless_force() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let bak_dir = tmp.path().join("bak");
        fs::create_dir_all(&src_dir).unwrap();
        let src = write_src(&src_dir, "a\n");

        let morning = Local
            .with_ymd_and_hms(2026, 7, 15, 9, 0, 0)
            .single()
            .unwrap();
        let afternoon = Local
            .with_ymd_and_hms(2026, 7, 15, 18, 0, 0)
            .single()
            .unwrap();

        let r1 = backup_at_paths(&src, &bak_dir, morning, false, 50, false).unwrap();
        assert!(r1.backed_up);

        let r2 = backup_at_paths(&src, &bak_dir, afternoon, false, 50, false).unwrap();
        assert!(!r2.backed_up);
        assert_eq!(r2.skipped_reason.as_deref(), Some("already_today"));

        let r3 = backup_at_paths(&src, &bak_dir, afternoon, true, 50, false).unwrap();
        assert!(r3.backed_up);
        assert_eq!(list_backup_names(&bak_dir).len(), 2);
    }

    #[test]
    fn test_missing_source_force_errors_soft_skips() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope.jsonl");
        let bak = tmp.path().join("bak");
        let now = Local::now();

        let soft = backup_at_paths(&missing, &bak, now, false, 50, false).unwrap();
        assert!(!soft.backed_up);
        assert_eq!(soft.skipped_reason.as_deref(), Some("source_missing"));

        let hard = backup_at_paths(&missing, &bak, now, true, 50, false);
        assert!(hard.is_err());
        assert!(hard.unwrap_err().contains("不存在"));
    }

    #[test]
    fn test_prune_keeps_newest() {
        let tmp = tempfile::tempdir().unwrap();
        let bak = tmp.path().join("bak");
        fs::create_dir_all(&bak).unwrap();
        for i in 1..=5 {
            let name = format!("requests-2026071{}-120000.jsonl", i);
            fs::write(bak.join(&name), format!("{}\n", i)).unwrap();
        }
        let removed = prune_backups(&bak, 3).unwrap();
        assert_eq!(removed, 2);
        let left = list_backup_names(&bak);
        assert_eq!(left.len(), 3);
        assert_eq!(left[0], "requests-20260713-120000.jsonl");
        assert_eq!(left[2], "requests-20260715-120000.jsonl");
    }

    #[test]
    fn test_list_backup_stats_from_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let bak = tmp.path().join("bak");
        fs::create_dir_all(&bak).unwrap();
        fs::write(bak.join("requests-20260715-143045.jsonl"), "x\n").unwrap();
        fs::write(bak.join("noise.txt"), "y\n").unwrap();
        let (count, last) = list_backup_stats(&bak);
        assert_eq!(count, 1);
        let expected = Local
            .with_ymd_and_hms(2026, 7, 15, 14, 30, 45)
            .single()
            .unwrap()
            .timestamp();
        assert_eq!(last, Some(expected));
    }

    #[test]
    fn test_has_backup_for_day() {
        let tmp = tempfile::tempdir().unwrap();
        let bak = tmp.path().join("bak");
        fs::create_dir_all(&bak).unwrap();
        assert!(!has_backup_for_day(&bak, "20260715"));
        fs::write(bak.join("requests-20260715-010203.jsonl"), "x\n").unwrap();
        assert!(has_backup_for_day(&bak, "20260715"));
        assert!(!has_backup_for_day(&bak, "20260716"));
    }
}
