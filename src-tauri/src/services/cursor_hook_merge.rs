//! 备份后归整 `requests.jsonl`：最小字段 + 同模型短窗内可折叠事件合并为一条。

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;
#[cfg(windows)]
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
#[cfg(windows)]
const FILE_SHARE_DELETE: u32 = 0x0000_0004;

const RENAME_RETRIES: u32 = 8;
const RENAME_RETRY_BASE_MS: u64 = 50;

use crate::services::cursor_attribution::normalize_model_family;
use crate::services::cursor_local_hook::{hook_row_model, hook_row_ts_epoch};

/// 同模型、可折叠事件在此秒数内视为同一 burst，只保留首条。
pub const MERGE_COLLAPSE_SECS: i64 = 10;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookMergeResult {
    pub merged: bool,
    pub rows_before: usize,
    pub rows_after: usize,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EventKind {
    Anchor,
    Collapsible,
}

fn event_kind(name: &str) -> EventKind {
    match name {
        "preToolUse" | "postToolUse" | "postToolUseFailure" | "beforeReadFile"
        | "afterFileEdit" | "afterAgentThought" | "beforeTabFileRead" => EventKind::Collapsible,
        _ => EventKind::Anchor,
    }
}

/// 将任意历史行压成最小字段集（与 log_request.ps1 写入格式一致）。
fn normalize_row(row: &Value) -> Value {
    let mut out = json!({});

    if let Some(s) = row.get("ts_utc").and_then(|v| v.as_str()).map(str::trim) {
        if !s.is_empty() {
            out["ts_utc"] = json!(s);
        }
    } else if let Some(s) = row.get("ts").and_then(|v| v.as_str()).map(str::trim) {
        if !s.is_empty() {
            out["ts_utc"] = json!(s);
        }
    }

    if let Some(s) = row.get("hook_event_name").and_then(|v| v.as_str()).map(str::trim) {
        if !s.is_empty() {
            out["hook_event_name"] = json!(s);
        }
    }

    let model = hook_row_model(row);
    if !model.is_empty() {
        out["model"] = json!(model);
    }
    if let Some(mid) = row.get("model_id").and_then(|v| v.as_str()).map(str::trim) {
        if !mid.is_empty() && mid != model {
            out["model_id"] = json!(mid);
        }
    }

    if row.get("_parse_error").and_then(|v| v.as_bool()) == Some(true)
        || row.get("_empty_stdin").and_then(|v| v.as_bool()) == Some(true)
    {
        out["_parse_error"] = json!(true);
        if let Some(m) = row.get("_parse_msg").and_then(|v| v.as_str()) {
            out["_parse_msg"] = json!(m);
        } else if row.get("_empty_stdin").is_some() {
            out["_parse_msg"] = json!("empty stdin");
        }
    }

    out
}

struct CollapseState {
    family: String,
    anchor_ts: i64,
}

fn serialize_rows(rows: &[Value]) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    for row in rows {
        let line = serde_json::to_string(row).map_err(|e| e.to_string())?;
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    Ok(body)
}

/// rename 覆盖目标；Windows 上 Hook 可能正追加，故带退避重试。
fn try_rename_with_retry(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    let mut last_err = None;
    for attempt in 0..RENAME_RETRIES {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt + 1 < RENAME_RETRIES {
                    thread::sleep(Duration::from_millis(
                        RENAME_RETRY_BASE_MS * u64::from(attempt + 1),
                    ));
                }
            }
        }
    }
    Err(last_err.unwrap())
}

/// 保留原 inode / ACL，避免 Windows 上 rename 替换导致 Hook 后续追加失败。
fn in_place_rewrite(path: &Path, body: &[u8]) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(path)
            .map_err(|e| format!("原地写回归整结果失败: {}", e))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("原地写回归整结果失败: {}", e))?;
        file.write_all(body)
            .map_err(|e| format!("原地写回归整结果失败: {}", e))?;
        file.set_len(body.len() as u64)
            .map_err(|e| format!("原地写回归整结果失败: {}", e))?;
        file.sync_all()
            .map_err(|e| format!("原地写回归整结果失败: {}", e))?;
    }

    #[cfg(not(windows))]
    {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)
            .map_err(|e| format!("原地写回归整结果失败: {}", e))?;
        file.write_all(body)
            .map_err(|e| format!("原地写回归整结果失败: {}", e))?;
        file.sync_all()
            .map_err(|e| format!("原地写回归整结果失败: {}", e))?;
    }

    Ok(())
}

fn commit_merged_contents(path: &Path, tmp: &Path, body: &[u8]) -> Result<(), String> {
    if try_rename_with_retry(tmp, path).is_ok() {
        return Ok(());
    }
    in_place_rewrite(path, body)?;
    let _ = fs::remove_file(tmp);
    Ok(())
}

/// 模拟 Hook 侧 Add-Content 的追加打开，确认归整后仍可写。
fn verify_hook_log_writable(path: &Path) -> Result<(), String> {
    let mut opts = OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(windows)]
    opts.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    opts.open(path)
        .map_err(|e| format!("归整后日志不可写: {}", e))?;
    Ok(())
}

/// 读取 jsonl → 归一化 → 折叠 → 原子写回。须在备份完成后对源文件调用。
pub fn merge_requests_jsonl(path: &Path) -> Result<HookMergeResult, String> {
    if !path.is_file() {
        return Ok(HookMergeResult {
            merged: false,
            rows_before: 0,
            rows_after: 0,
            message: "源文件不存在，已跳过归整".into(),
        });
    }

    let file = File::open(path).map_err(|e| format!("读取 Hook 日志失败: {}", e))?;
    let reader = BufReader::new(file);
    let mut normalized: Vec<Value> = Vec::new();
    let mut rows_before = 0usize;

    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        rows_before += 1;
        let row: Value = match serde_json::from_str(line) {
            Ok(v) => normalize_row(&v),
            Err(_) => json!({
                "_parse_error": true,
                "_parse_msg": "invalid json line",
            }),
        };
        normalized.push(row);
    }

    if rows_before == 0 {
        return Ok(HookMergeResult {
            merged: false,
            rows_before: 0,
            rows_after: 0,
            message: "日志为空，已跳过归整".into(),
        });
    }

    let mut out_rows: Vec<Value> = Vec::with_capacity(normalized.len());
    let mut collapse: Option<CollapseState> = None;

    for row in normalized {
        let event_name = row
            .get("hook_event_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let model = hook_row_model(&row);
        let ts = hook_row_ts_epoch(&row);

        if event_kind(event_name) == EventKind::Anchor {
            out_rows.push(row);
            collapse = None;
            continue;
        }

        // 无模型 / 无时间：不参与折叠，原样保留
        if model.is_empty() || ts.is_none() {
            out_rows.push(row);
            collapse = None;
            continue;
        }

        let family = normalize_model_family(&model);
        let ts = ts.unwrap();

        let skip = collapse.as_ref().is_some_and(|st| {
            st.family == family && ts.saturating_sub(st.anchor_ts) <= MERGE_COLLAPSE_SECS
        });

        if skip {
            continue;
        }

        out_rows.push(row);
        collapse = Some(CollapseState {
            family,
            anchor_ts: ts,
        });
    }

    let rows_after = out_rows.len();
    let body = serialize_rows(&out_rows)?;
    let tmp = path.with_extension("jsonl.merge.tmp");
    {
        let mut f = File::create(&tmp).map_err(|e| format!("创建归整临时文件失败: {}", e))?;
        f.write_all(&body)
            .map_err(|e| format!("写入归整临时文件失败: {}", e))?;
        f.sync_all()
            .map_err(|e| format!("写入归整临时文件失败: {}", e))?;
    }
    commit_merged_contents(path, &tmp, &body)?;
    verify_hook_log_writable(path)?;

    let removed = rows_before.saturating_sub(rows_after);
    let merged = removed > 0;
    let message = if merged {
        format!(
            "归整完成：{} 行 → {} 行（合并 {} 条）",
            rows_before, rows_after, removed
        )
    } else {
        "归整完成：无重复可合并".into()
    };

    Ok(HookMergeResult {
        merged,
        rows_before,
        rows_after,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_merge_collapses_tool_burst_same_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("requests.jsonl");
        let content = r#"{"ts_utc":"2026-07-22T03:44:04+00:00","hook_event_name":"preToolUse","model":"composer-2.5"}
{"ts_utc":"2026-07-22T03:44:04+00:00","hook_event_name":"preToolUse","model":"composer-2.5"}
{"ts_utc":"2026-07-22T03:44:06+00:00","hook_event_name":"postToolUse","model":"composer-2.5"}
{"ts_utc":"2026-07-22T03:44:46+00:00","hook_event_name":"stop","model":"composer-2.5"}
"#;
        fs::write(&path, content).unwrap();
        let r = merge_requests_jsonl(&path).unwrap();
        assert!(r.merged);
        assert_eq!(r.rows_before, 4);
        assert_eq!(r.rows_after, 2);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("preToolUse"));
        assert!(text.contains("stop"));
    }

    #[test]
    fn test_merge_does_not_cross_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("requests.jsonl");
        let content = r#"{"ts_utc":"2026-07-22T03:00:00+00:00","hook_event_name":"preToolUse","model":"composer-2.5"}
{"ts_utc":"2026-07-22T03:00:01+00:00","hook_event_name":"preToolUse","model":"cursor-grok-4.5-high"}
"#;
        fs::write(&path, content).unwrap();
        let r = merge_requests_jsonl(&path).unwrap();
        assert_eq!(r.rows_after, 2);
    }

    #[test]
    fn test_normalize_strips_extra_fields() {
        let row = json!({
            "ts_utc": "2026-07-22T03:00:00+00:00",
            "hook_event_name": "postToolUse",
            "model": "composer-2.5",
            "conversation_id": "abc",
            "tool_name": "Shell"
        });
        let n = normalize_row(&row);
        assert_eq!(n.get("conversation_id"), None);
        assert_eq!(n.get("tool_name"), None);
        assert_eq!(n.get("model").and_then(|v| v.as_str()), Some("composer-2.5"));
    }

    #[test]
    fn test_merge_result_still_writable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("requests.jsonl");
        fs::write(
            &path,
            r#"{"ts_utc":"2026-07-22T03:00:00+00:00","hook_event_name":"stop","model":"composer-2.5"}
"#,
        )
        .unwrap();
        merge_requests_jsonl(&path).unwrap();
        verify_hook_log_writable(&path).unwrap();
        let mut opts = OpenOptions::new();
        opts.append(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            opts.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
        }
        let mut f = opts.open(&path).unwrap();
        writeln!(f, r#"{{"ts_utc":"2026-07-22T03:01:00+00:00","hook_event_name":"stop","model":"composer-2.5"}}"#)
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
    }
}
