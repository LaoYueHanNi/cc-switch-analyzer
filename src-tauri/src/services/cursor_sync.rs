use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::services::cursor_csv::parse_date_to_epoch_secs;
use crate::services::cursor_local_hook::{read_setting_raw, write_setting_raw};
use crate::utils;

const CURSOR_HTTP_TIMEOUT: Duration = Duration::from_secs(8);
/// 自动同步缓存有效期：24 小时内不重复请求 Cursor API（手动同步不受限）
pub const CURSOR_AUTO_SYNC_FRESHNESS: Duration = Duration::from_secs(24 * 60 * 60);

const USAGE_CSV_BASE: &str = "https://cursor.com/api/dashboard/export-usage-events-csv";
const USAGE_SUMMARY_ENDPOINT: &str = "https://cursor.com/api/usage-summary";
pub const SYNC_LOOKBACK_SETTING_KEY: &str = "cursor_sync_lookback";

/// Cursor CSV 同步时间范围档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncLookback {
    Days(u32),
    All,
}

impl SyncLookback {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncLookback::Days(1) => "1d",
            SyncLookback::Days(7) => "7d",
            SyncLookback::Days(30) => "30d",
            SyncLookback::All => "all",
            SyncLookback::Days(_) => "7d",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "1d" | "1" => SyncLookback::Days(1),
            "30d" | "30" => SyncLookback::Days(30),
            "all" => SyncLookback::All,
            "7d" | "7" => SyncLookback::Days(7),
            _ => SyncLookback::Days(7),
        }
    }
}

pub fn get_sync_lookback() -> SyncLookback {
    read_setting_raw(SYNC_LOOKBACK_SETTING_KEY)
        .map(|v| SyncLookback::parse(&v))
        .unwrap_or(SyncLookback::Days(7))
}

pub fn set_sync_lookback(lookback: &str) -> Result<SyncLookback, String> {
    let parsed = SyncLookback::parse(lookback);
    write_setting_raw(SYNC_LOOKBACK_SETTING_KEY, parsed.as_str())?;
    Ok(parsed)
}

fn beijing_tz() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("UTC+8")
}

/// 按北京日历日计算拉取窗口。
/// 返回 `(start_ms, end_ms, start_epoch_secs)`；`All` 返回 `None`。
pub fn lookback_window_ms(
    lookback: SyncLookback,
    now_utc: DateTime<Utc>,
) -> Option<(i64, i64, i64)> {
    let days = match lookback {
        SyncLookback::All => return None,
        SyncLookback::Days(n) => n.max(1),
    };
    let bj = now_utc.with_timezone(&beijing_tz());
    let today = bj.date_naive();
    let start_date = today - chrono::Duration::days((days as i64) - 1);
    let start_naive = start_date
        .and_hms_milli_opt(0, 0, 0, 0)
        .expect("valid start midnight");
    let end_naive = today
        .and_hms_milli_opt(23, 59, 59, 999)
        .expect("valid end of day");
    let start_bj = beijing_tz()
        .from_local_datetime(&start_naive)
        .single()
        .expect("beijing start");
    let end_bj = beijing_tz()
        .from_local_datetime(&end_naive)
        .single()
        .expect("beijing end");
    Some((
        start_bj.timestamp_millis(),
        end_bj.timestamp_millis(),
        start_bj.timestamp(),
    ))
}

fn build_usage_csv_url(lookback: SyncLookback) -> (String, Option<i64>) {
    match lookback_window_ms(lookback, Utc::now()) {
        None => (format!("{}?strategy=tokens", USAGE_CSV_BASE), None),
        Some((start_ms, end_ms, start_epoch)) => (
            format!(
                "{}?startDate={}&endDate={}&strategy=tokens",
                USAGE_CSV_BASE, start_ms, end_ms
            ),
            Some(start_epoch),
        ),
    }
}

/// 将本次拉取的 CSV 与本地缓存合并：保留 `created_at < start_epoch` 的旧行，窗口内用新数据。
pub fn merge_usage_csv(existing: Option<&str>, fetched: &str, start_epoch: i64) -> String {
    let fetched = fetched.trim_start_matches('\u{feff}');
    let mut fetched_lines = fetched.lines();
    let Some(new_header) = fetched_lines.next() else {
        return fetched.to_string();
    };
    let new_rows: Vec<&str> = fetched_lines.filter(|l| !l.trim().is_empty()).collect();

    let mut kept_old: Vec<String> = Vec::new();
    if let Some(old) = existing {
        let old = old.trim_start_matches('\u{feff}');
        let mut old_lines = old.lines();
        let Some(old_header) = old_lines.next() else {
            return format!("{}\n{}", new_header, new_rows.join("\n"));
        };
        if !old_header.contains("Date") {
            return format!("{}\n{}", new_header, new_rows.join("\n"));
        }
        for line in old_lines {
            if line.trim().is_empty() {
                continue;
            }
            let date_field = line.split(',').next().unwrap_or("").trim().trim_matches('"');
            let ts = parse_date_to_epoch_secs(date_field);
            if ts > 0 && ts < start_epoch {
                kept_old.push(line.to_string());
            }
        }
    }

    let mut out = String::with_capacity(fetched.len() + kept_old.iter().map(|s| s.len() + 1).sum::<usize>());
    out.push_str(new_header);
    out.push('\n');
    for line in &kept_old {
        out.push_str(line);
        out.push('\n');
    }
    for (i, line) in new_rows.iter().enumerate() {
        out.push_str(line);
        if i + 1 < new_rows.len() || !kept_old.is_empty() {
            // always newline between rows; final newline ok
        }
        out.push('\n');
    }
    // trim trailing newline for consistency with typical CSV? keep trailing newline
    if out.ends_with('\n') && !new_rows.is_empty() {
        // fine
    }
    out
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorCredentials {
    #[serde(rename = "sessionToken")]
    pub session_token: String,
    #[serde(rename = "userId", skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCursorResult {
    pub synced: bool,
    pub rows: usize,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateSessionResult {
    pub valid: bool,
    pub membership_type: Option<String>,
    pub error: Option<String>,
}

fn build_http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(CURSOR_HTTP_TIMEOUT))
        .max_redirects(5)
        .build()
        .into()
}

fn extract_user_id_from_session_token(token: &str) -> Option<String> {
    let token = token.trim();
    let user_id = if token.contains("%3A%3A") {
        token.split("%3A%3A").next()?
    } else if token.contains("::") {
        token.split("::").next()?
    } else {
        return None;
    };
    let user_id = user_id.trim();
    if user_id.is_empty() {
        None
    } else {
        Some(user_id.to_string())
    }
}

fn atomic_write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    let temp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp_path)
            .map_err(|e| format!("创建临时文件失败: {}", e))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
    }
    if let Err(err) = fs::rename(&temp_path, path) {
        if path.exists() {
            fs::copy(&temp_path, path).map_err(|copy_err| {
                let _ = fs::remove_file(&temp_path);
                format!("持久化文件失败: rename={}, copy={}", err, copy_err)
            })?;
            let _ = fs::remove_file(&temp_path);
        } else {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("持久化文件失败: {}", err));
        }
    }
    Ok(())
}

pub fn load_credentials() -> Option<CursorCredentials> {
    let path = utils::get_cursor_credentials_path().ok()?;
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_credentials(session_token: &str) -> Result<CursorCredentials, String> {
    let token = session_token.trim();
    if token.is_empty() {
        return Err("Session Token 不能为空".to_string());
    }
    let creds = CursorCredentials {
        session_token: token.to_string(),
        user_id: extract_user_id_from_session_token(token),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let path = utils::get_cursor_credentials_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {}", e))?;
    }
    let json = serde_json::to_string_pretty(&creds)
        .map_err(|e| format!("序列化凭证失败: {}", e))?;
    atomic_write_file(&path, &json)?;
    Ok(creds)
}

pub fn clear_credentials() -> Result<(), String> {
    let path = utils::get_cursor_credentials_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("删除凭证失败: {}", e))?;
    }
    Ok(())
}

pub fn is_logged_in() -> bool {
    load_credentials().is_some()
}

fn mask_token(token: &str) -> String {
    let t = token.trim();
    if t.len() <= 16 {
        return "***".to_string();
    }
    format!("{}...{}(len={})", &t[..8], &t[t.len().saturating_sub(6)..], t.len())
}

pub fn validate_cursor_session(session_token: &str) -> ValidateSessionResult {
    let agent = build_http_agent();
    log::info!(
        "[CURSOR] validate session GET {} token={}",
        USAGE_SUMMARY_ENDPOINT,
        mask_token(session_token)
    );

    let response = match agent
        .get(USAGE_SUMMARY_ENDPOINT)
        .header("Cookie", &format!("WorkosCursorSessionToken={}", session_token.trim()))
        .header("Referer", "https://www.cursor.com/settings")
        .header("Accept", "*/*")
        .call()
    {
        Ok(resp) => {
            log::info!(
                "[CURSOR] validate session OK status={}",
                resp.status()
            );
            resp
        }
        Err(ureq::Error::StatusCode(code)) => {
            log::warn!("[CURSOR] validate session HTTP {}", code);
            if code == 401 || code == 403 {
                return ValidateSessionResult {
                    valid: false,
                    membership_type: None,
                    error: Some("Session Token 已过期或无效".to_string()),
                };
            }
            return ValidateSessionResult {
                valid: false,
                membership_type: None,
                error: Some(format!("Cursor API 返回状态 {}", code)),
            };
        }
        Err(e) => {
            log::warn!("[CURSOR] validate session request failed: {}", e);
            return ValidateSessionResult {
                valid: false,
                membership_type: None,
                error: Some(format!("请求失败: {}", e)),
            };
        }
    };

    let body = match response.into_body().read_to_string() {
        Ok(body) => body,
        Err(e) => {
            log::warn!("[CURSOR] validate session read body failed: {}", e);
            return ValidateSessionResult {
                valid: false,
                membership_type: None,
                error: Some(format!("读取响应失败: {}", e)),
            };
        }
    };
    log::info!("[CURSOR] validate session body_len={}", body.len());

    let data: serde_json::Value = match serde_json::from_str(&body) {
        Ok(data) => data,
        Err(e) => {
            log::warn!(
                "[CURSOR] validate session JSON parse failed: {} body_prefix={:?}",
                e,
                body.chars().take(120).collect::<String>()
            );
            return ValidateSessionResult {
                valid: false,
                membership_type: None,
                error: Some(format!("解析响应失败: {}", e)),
            };
        }
    };

    let has_billing_start = data
        .get("billingCycleStart")
        .and_then(|v| v.as_str())
        .is_some();
    let has_billing_end = data
        .get("billingCycleEnd")
        .and_then(|v| v.as_str())
        .is_some();

    if has_billing_start && has_billing_end {
        let membership = data
            .get("membershipType")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        log::info!(
            "[CURSOR] validate session valid=true membership={:?}",
            membership
        );
        ValidateSessionResult {
            valid: true,
            membership_type: membership,
            error: None,
        }
    } else {
        log::warn!(
            "[CURSOR] validate session invalid body keys={:?}",
            data.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
        ValidateSessionResult {
            valid: false,
            membership_type: None,
            error: Some("Cursor API 响应格式无效".to_string()),
        }
    }
}

pub fn fetch_cursor_usage_csv(session_token: &str) -> Result<(String, Option<i64>), String> {
    let lookback = get_sync_lookback();
    let (url, start_epoch) = build_usage_csv_url(lookback);
    let agent = build_http_agent();
    let started = std::time::Instant::now();
    log::info!(
        "[CURSOR] fetch CSV GET {} lookback={} token={} timeout={}s",
        url,
        lookback.as_str(),
        mask_token(session_token),
        CURSOR_HTTP_TIMEOUT.as_secs()
    );
    let response = agent
        .get(&url)
        .header("Cookie", &format!("WorkosCursorSessionToken={}", session_token.trim()))
        .header("Referer", "https://www.cursor.com/settings")
        .header("Accept", "*/*")
        .call()
        .map_err(|e| {
            let msg = match &e {
                ureq::Error::StatusCode(code) if *code == 401 || *code == 403 => {
                    "Cursor Session Token 已过期，请重新登录".to_string()
                }
                other => format!("Cursor API 请求失败: {}", other),
            };
            log::warn!(
                "[CURSOR] fetch CSV failed after {:?}: {}",
                started.elapsed(),
                msg
            );
            msg
        })?;

    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    log::info!(
        "[CURSOR] fetch CSV response status={} content-type={} elapsed={:?}",
        status,
        content_type,
        started.elapsed()
    );

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| {
            let msg = format!("读取 Cursor CSV 响应失败: {}", e);
            log::warn!("[CURSOR] {}", msg);
            msg
        })?;

    let rows = count_cursor_csv_rows(&body);
    log::info!(
        "[CURSOR] fetch CSV body_len={} rows={} prefix={:?}",
        body.len(),
        rows,
        body.chars().take(60).collect::<String>()
    );

    if !body.trim_start_matches('\u{feff}').starts_with("Date,") {
        log::warn!(
            "[CURSOR] fetch CSV non-CSV body prefix={:?}",
            body.chars().take(160).collect::<String>()
        );
        return Err("Cursor API 返回非 CSV 格式".to_string());
    }
    Ok((body, start_epoch))
}

pub fn count_cursor_csv_rows(csv_text: &str) -> usize {
    csv_text.lines().skip(1).filter(|l| !l.trim().is_empty()).count()
}

pub fn cache_is_fresh() -> bool {
    let Ok(path) = utils::get_cursor_usage_csv_path() else {
        return false;
    };
    let Ok(meta) = fs::metadata(&path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|elapsed| elapsed < CURSOR_AUTO_SYNC_FRESHNESS)
        .unwrap_or(false)
}

pub fn cache_last_modified() -> Option<SystemTime> {
    let path = utils::get_cursor_usage_csv_path().ok()?;
    fs::metadata(&path).ok()?.modified().ok()
}

pub fn sync_cursor_cache() -> SyncCursorResult {
    let creds = match load_credentials() {
        Some(c) => c,
        None => {
            log::warn!("[CURSOR] sync aborted: no credentials");
            return SyncCursorResult {
                synced: false,
                rows: 0,
                error: Some("未登录 Cursor".to_string()),
            };
        }
    };
    log::info!(
        "[CURSOR] sync start user_id={:?} creds_created_at={} token={}",
        creds.user_id,
        creds.created_at,
        mask_token(&creds.session_token)
    );

    match fetch_cursor_usage_csv(&creds.session_token) {
        Ok((csv_text, start_epoch)) => {
            let cache_dir = match utils::get_cursor_cache_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    log::warn!("[CURSOR] sync get cache dir failed: {}", e);
                    return SyncCursorResult {
                        synced: false,
                        rows: 0,
                        error: Some(e),
                    };
                }
            };
            if let Err(e) = fs::create_dir_all(&cache_dir) {
                let msg = format!("创建缓存目录失败: {}", e);
                log::warn!("[CURSOR] {}", msg);
                return SyncCursorResult {
                    synced: false,
                    rows: 0,
                    error: Some(msg),
                };
            }
            let file_path = cache_dir.join("usage.csv");
            let existing = fs::read_to_string(&file_path).ok();
            let merged = match start_epoch {
                Some(start) => {
                    let out = merge_usage_csv(existing.as_deref(), &csv_text, start);
                    log::info!(
                        "[CURSOR] merge lookback={} start_epoch={} fetched_rows={} merged_rows={}",
                        get_sync_lookback().as_str(),
                        start,
                        count_cursor_csv_rows(&csv_text),
                        count_cursor_csv_rows(&out)
                    );
                    out
                }
                None => {
                    log::info!(
                        "[CURSOR] full overwrite lookback=all fetched_rows={}",
                        count_cursor_csv_rows(&csv_text)
                    );
                    csv_text
                }
            };
            let row_count = count_cursor_csv_rows(&merged);
            if let Err(e) = atomic_write_file(&file_path, &merged) {
                log::warn!("[CURSOR] sync write cache failed: {}", e);
                return SyncCursorResult {
                    synced: false,
                    rows: 0,
                    error: Some(e),
                };
            }
            log::info!(
                "[CURSOR] sync OK rows={} path={}",
                row_count,
                file_path.display()
            );
            SyncCursorResult {
                synced: true,
                rows: row_count,
                error: None,
            }
        }
        Err(e) => {
            log::warn!("[CURSOR] sync failed: {}", e);
            SyncCursorResult {
                synced: false,
                rows: 0,
                error: Some(e),
            }
        }
    }
}

/// 缓存过期时自动同步，返回是否执行了同步
pub fn maybe_auto_sync() -> Result<bool, String> {
    if !is_logged_in() {
        log::debug!("[CURSOR] auto-sync skip: not logged in");
        return Ok(false);
    }
    if cache_is_fresh() {
        let age = cache_last_modified()
            .and_then(|t| t.elapsed().ok())
            .map(|d| format!("{}s", d.as_secs()))
            .unwrap_or_else(|| "unknown".into());
        log::info!(
            "[CURSOR] auto-sync skip: cache fresh (age≈{}, ttl={}h)",
            age,
            CURSOR_AUTO_SYNC_FRESHNESS.as_secs() / 3600
        );
        return Ok(false);
    }
    log::info!("[CURSOR] auto-sync triggered: cache stale or missing");
    let result = sync_cursor_cache();
    if result.synced {
        Ok(true)
    } else if let Some(err) = result.error {
        log::warn!("[CURSOR] 自动同步失败: {}", err);
        Ok(false)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn lookback_parse_defaults() {
        assert_eq!(SyncLookback::parse("7d"), SyncLookback::Days(7));
        assert_eq!(SyncLookback::parse("1d"), SyncLookback::Days(1));
        assert_eq!(SyncLookback::parse("30d"), SyncLookback::Days(30));
        assert_eq!(SyncLookback::parse("all"), SyncLookback::All);
        assert_eq!(SyncLookback::parse("ALL"), SyncLookback::All);
        assert_eq!(SyncLookback::parse("bogus"), SyncLookback::Days(7));
    }

    #[test]
    fn beijing_7d_window_from_fixed_now() {
        // 北京 2026-07-14 15:00 = UTC 2026-07-14 07:00
        let now = Utc.with_ymd_and_hms(2026, 7, 14, 7, 0, 0).unwrap();
        let (start_ms, end_ms, start_epoch) =
            lookback_window_ms(SyncLookback::Days(7), now).unwrap();
        // start: 2026-07-08 00:00:00 +08 = 2026-07-07 16:00:00 UTC
        let expect_start = beijing_tz()
            .with_ymd_and_hms(2026, 7, 8, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let expect_end = beijing_tz()
            .with_ymd_and_hms(2026, 7, 14, 23, 59, 59)
            .unwrap()
            .timestamp_millis()
            + 999; // 23:59:59.999
        assert_eq!(start_ms, expect_start);
        assert_eq!(end_ms, expect_end);
        assert_eq!(start_epoch, expect_start / 1000);
        assert!(lookback_window_ms(SyncLookback::All, now).is_none());
    }

    #[test]
    fn beijing_1d_is_today_only() {
        let now = Utc.with_ymd_and_hms(2026, 7, 14, 7, 0, 0).unwrap();
        let (start_ms, end_ms, _) = lookback_window_ms(SyncLookback::Days(1), now).unwrap();
        let expect_start = beijing_tz()
            .with_ymd_and_hms(2026, 7, 14, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        assert_eq!(start_ms, expect_start);
        assert!(end_ms > start_ms);
    }

    #[test]
    fn merge_keeps_rows_before_window() {
        let header = "Date,Cloud Agent ID,Automation ID,Kind,Model,Max Mode,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost";
        let old_outside = "2026-07-01T01:00:00.000Z,,,,,,,0,10,0,5,15,Included";
        let old_inside = "2026-07-10T01:00:00.000Z,,,,,,,0,20,0,5,25,Included";
        let existing = format!("{}\n{}\n{}\n", header, old_outside, old_inside);
        let fetched = format!(
            "{}\n{}\n",
            header, "2026-07-12T02:00:00.000Z,,,,,,,0,30,0,5,35,Included"
        );
        // 2026-07-08 00:00 +08
        let start_epoch = beijing_tz()
            .with_ymd_and_hms(2026, 7, 8, 0, 0, 0)
            .unwrap()
            .timestamp();
        let merged = merge_usage_csv(Some(&existing), &fetched, start_epoch);
        assert!(merged.contains("2026-07-01T01:00:00.000Z"));
        assert!(!merged.contains("2026-07-10T01:00:00.000Z"));
        assert!(merged.contains("2026-07-12T02:00:00.000Z"));
        assert_eq!(count_cursor_csv_rows(&merged), 2);
    }

    #[test]
    fn merge_without_existing_uses_fetched() {
        let fetched = "Date,Model\n2026-07-12T02:00:00.000Z,composer-2.5\n";
        let merged = merge_usage_csv(None, fetched, 0);
        assert_eq!(merged.lines().next().unwrap(), "Date,Model");
        assert!(merged.contains("composer-2.5"));
    }
}
