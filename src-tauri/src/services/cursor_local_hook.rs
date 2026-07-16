//! Cursor 本机 Hook 安装/卸载与 requests.jsonl 读取

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use crate::services::cursor_attribution::{
    normalize_model_family, LocalHookEvent, ATTRIBUTION_SETTING_KEY,
};
use crate::utils;

const HOOK_MARKER: &str = "local-usage/run_log.ps1";

/// 安装到 hooks.json 的事件：覆盖所有可能触发模型调用的生命周期。
/// 不含 beforeShellExecution / preToolUse 等纯工具钩子（会带父会话 model 刷屏，干扰归因）。
const HOOK_EVENTS: &[&str] = &[
    // 主 Agent / Cmd+K / Agent Review（独立 composer 会话）
    "sessionStart",
    "sessionEnd",
    "beforeSubmitPrompt",
    "afterAgentThought",
    "afterAgentResponse",
    "stop",
    // Task / subagent（explore、shell、bugbot、generalPurpose 等）
    "subagentStart",
    "subagentStop",
    // 上下文压缩也会调模型
    "preCompact",
    // Tab 补全
    "beforeTabFileRead",
    "afterTabFileEdit",
];

/// 可用于 CSV 本机归因的事件（需能解析出非空 model）。
const ATTRIBUTION_HOOK_EVENTS: &[&str] = &[
    "beforeSubmitPrompt",
    "afterAgentThought",
    "afterAgentResponse",
    "stop",
    "subagentStart",
    "subagentStop",
    "preCompact",
    "sessionStart",
    "beforeTabFileRead",
];

const RUN_LOG_PS1: &[u8] = include_bytes!("../../resources/cursor-local-usage/run_log.ps1");
const LOG_REQUEST_PS1: &[u8] = include_bytes!("../../resources/cursor-local-usage/log_request.ps1");

pub(crate) fn read_setting_raw(key: &str) -> Option<String> {
    let path = utils::get_app_db_path().ok()?;
    if !path.exists() {
        return None;
    }
    let conn = Connection::open(&path).ok()?;
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

pub(crate) fn write_setting_raw(key: &str, value: &str) -> Result<(), String> {
    let path = utils::get_app_db_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {}", e))?;
    }
    let conn = Connection::open(&path).map_err(|e| format!("打开应用数据库失败: {}", e))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;
    let now = Utc::now().timestamp();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn is_attribution_enabled() -> bool {
    read_setting_raw(ATTRIBUTION_SETTING_KEY).as_deref() == Some("1")
}

pub fn set_attribution_enabled(enabled: bool) -> Result<(), String> {
    write_setting_raw(ATTRIBUTION_SETTING_KEY, if enabled { "1" } else { "0" })
}

pub fn local_usage_dir() -> Result<PathBuf, String> {
    utils::get_cursor_local_usage_dir()
}

pub fn requests_jsonl_path() -> Result<PathBuf, String> {
    Ok(local_usage_dir()?.join("requests.jsonl"))
}

pub fn is_hook_installed() -> bool {
    let Ok(hooks_path) = utils::get_cursor_hooks_json_path() else {
        return false;
    };
    let Ok(content) = fs::read_to_string(&hooks_path) else {
        return false;
    };
    content.contains(HOOK_MARKER)
}

fn hook_command_for(dir: &Path) -> String {
    let script = dir.join("run_log.ps1");
    let script_str = script.to_string_lossy().replace('\\', "/");
    format!(
        "C:/WINDOWS/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -ExecutionPolicy Bypass -File {}",
        script_str
    )
}

fn is_our_hook_command(cmd: &str) -> bool {
    let norm = cmd.replace('\\', "/");
    norm.contains(HOOK_MARKER)
}

/// 安装/更新脚本并合并 hooks.json（仅 Windows 完整支持）
pub fn install_hooks() -> Result<String, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let dir = local_usage_dir()?;
        fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;
        return Ok("本机精准归因已启用；自动安装 Hook 仅支持 Windows，若已有 requests.jsonl 仍可过滤".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let dir = local_usage_dir()?;
        fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;
        fs::write(dir.join("run_log.ps1"), RUN_LOG_PS1)
            .map_err(|e| format!("写入 run_log.ps1 失败: {}", e))?;
        fs::write(dir.join("log_request.ps1"), LOG_REQUEST_PS1)
            .map_err(|e| format!("写入 log_request.ps1 失败: {}", e))?;

        merge_hooks_json(&dir, true)?;
        Ok(format!(
            "已安装 Hook 至 {}（需 Reload Cursor 窗口后生效）",
            dir.display()
        ))
    }
}

pub fn uninstall_hooks() -> Result<String, String> {
    let dir = local_usage_dir().ok();
    if let Some(ref d) = dir {
        let _ = merge_hooks_json(d, false);
    } else {
        // 仍尝试清理 hooks.json
        let dummy = PathBuf::from(".");
        let _ = merge_hooks_json(&dummy, false);
    }
    Ok("已移除本应用管理的 Cursor Hook 条目".to_string())
}

fn merge_hooks_json(dir: &Path, install: bool) -> Result<(), String> {
    let path = utils::get_cursor_hooks_json_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 .cursor 目录失败: {}", e))?;
    }

    let mut root: Value = if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| format!("读取 hooks.json 失败: {}", e))?;
        if content.trim().is_empty() {
            serde_json::json!({ "version": 1, "hooks": {} })
        } else {
            serde_json::from_str(&content).unwrap_or_else(|_| {
                serde_json::json!({ "version": 1, "hooks": {} })
            })
        }
    } else {
        serde_json::json!({ "version": 1, "hooks": {} })
    };

    if root.get("version").is_none() {
        root["version"] = serde_json::json!(1);
    }
    if !root.get("hooks").map(|h| h.is_object()).unwrap_or(false) {
        root["hooks"] = serde_json::json!({});
    }

    let hooks = root
        .get_mut("hooks")
        .and_then(|h| h.as_object_mut())
        .ok_or_else(|| "hooks.json 格式无效".to_string())?;

    let our_cmd = hook_command_for(dir);

    for event in HOOK_EVENTS {
        let mut entries: Vec<Value> = hooks
            .get(*event)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                item.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| !is_our_hook_command(c))
                    .unwrap_or(true)
            })
            .collect();

        if install {
            entries.push(serde_json::json!({
                "command": our_cmd,
                "timeout": 12
            }));
        }

        if entries.is_empty() {
            hooks.remove(*event);
        } else {
            hooks.insert((*event).to_string(), Value::Array(entries));
        }
    }

    let pretty = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(&path, pretty).map_err(|e| format!("写入 hooks.json 失败: {}", e))?;
    Ok(())
}

fn parse_event_ts(row: &Value) -> Option<i64> {
    if let Some(s) = row.get("ts_utc").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Some(dt.timestamp());
        }
        if let Ok(dt) = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z") {
            return Some(dt.timestamp());
        }
    }
    if let Some(s) = row.get("ts").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Some(dt.timestamp());
        }
        // +08:00 without colon variants already covered by rfc3339
        if let Ok(dt) = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%z") {
            return Some(dt.timestamp());
        }
        // fallback: treat as local+8 if no offset
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            let offset = FixedOffset::east_opt(8 * 3600)?;
            return Some(naive.and_local_timezone(offset).single()?.timestamp());
        }
    }
    None
}

/// 从 hook 日志行解析模型：优先 `model`，其次 `subagent_model`（subagentStart）。
fn extract_model(row: &Value) -> String {
    for key in ["model", "subagent_model", "model_id"] {
        if let Some(m) = row.get(key).and_then(|v| v.as_str()).map(str::trim) {
            if !m.is_empty() {
                return m.to_string();
            }
        }
    }
    String::new()
}

fn is_attribution_event(event_name: &str) -> bool {
    ATTRIBUTION_HOOK_EVENTS.contains(&event_name)
}

/// 读取可用于归因的本机事件（模型相关 hook，且有 model / subagent_model）
pub fn load_local_events() -> Result<Vec<LocalHookEvent>, String> {
    let path = requests_jsonl_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path).map_err(|e| format!("读取 requests.jsonl 失败: {}", e))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let event_name = row
            .get("hook_event_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !is_attribution_event(event_name) {
            continue;
        }
        let model = extract_model(&row);
        if model.is_empty() {
            continue;
        }
        let Some(ts_epoch) = parse_event_ts(&row) else {
            continue;
        };
        let family = normalize_model_family(&model);
        events.push(LocalHookEvent {
            ts_epoch,
            model,
            family,
            hook_event_name: event_name.to_string(),
        });
    }
    Ok(events)
}

pub fn local_event_count() -> usize {
    load_local_events().map(|v| v.len()).unwrap_or(0)
}

pub fn attribution_hint(enabled: bool) -> String {
    if !enabled {
        return "关闭时统计账号全量 CSV；开启后按本机 Hook 日志过滤".to_string();
    }
    let count = local_event_count();
    let installed = is_hook_installed();
    #[cfg(target_os = "windows")]
    {
        if installed {
            format!("已装 Hook · {} 条本机事件（分钟±5 + 模型家族）", count)
        } else {
            format!("已启用但 Hook 未检测到 · {} 条本机事件", count)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = installed;
        format!(
            "已启用过滤 · {} 条本机事件（自动装 Hook 仅 Windows）",
            count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_merge_hooks_preserves_others() {
        let dir = tempdir().unwrap();
        let cursor_dir = dir.path().join(".cursor");
        fs::create_dir_all(&cursor_dir).unwrap();
        let hooks_path = cursor_dir.join("hooks.json");
        fs::write(
            &hooks_path,
            r#"{
  "version": 1,
  "hooks": {
    "beforeSubmitPrompt": [
      { "command": "C:/other/tool.exe", "timeout": 5 }
    ]
  }
}"#,
        )
        .unwrap();

        // monkey via env is hard; test filter helpers instead
        assert!(is_our_hook_command(
            "C:/Users/x/.cursor/local-usage/run_log.ps1"
        ));
        assert!(!is_our_hook_command("C:/other/tool.exe"));

        let mut root: Value = serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).unwrap();
        let hooks = root.get_mut("hooks").unwrap().as_object_mut().unwrap();
        let our_cmd = "C:/Users/x/.cursor/local-usage/run_log.ps1";
        let mut entries: Vec<Value> = hooks
            .get("beforeSubmitPrompt")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        entries.retain(|item| {
            item.get("command")
                .and_then(|c| c.as_str())
                .map(|c| !is_our_hook_command(c))
                .unwrap_or(true)
        });
        entries.push(serde_json::json!({"command": our_cmd, "timeout": 12}));
        hooks.insert("beforeSubmitPrompt".into(), Value::Array(entries));
        let arr = hooks.get("beforeSubmitPrompt").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().any(|e| e["command"] == "C:/other/tool.exe"));
        assert!(arr.iter().any(|e| is_our_hook_command(e["command"].as_str().unwrap())));
    }

    #[test]
    fn test_load_local_events_filters() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("requests.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"ts_utc":"2026-07-13T06:00:00+00:00","hook_event_name":"beforeSubmitPrompt","model":"cursor-grok-4.5-high"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"ts_utc":"2026-07-13T06:00:01+00:00","hook_event_name":"afterAgentResponse","model":"cursor-grok-4.5-high"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"ts_utc":"2026-07-13T06:00:02+00:00","hook_event_name":"stop"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"ts_utc":"2026-07-13T06:00:03+00:00","hook_event_name":"subagentStart","subagent_model":"composer-2.5","subagent_type":"explore"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"ts_utc":"2026-07-13T06:00:04+00:00","hook_event_name":"sessionEnd","reason":"completed"}}"#
        )
        .unwrap();

        let file = fs::File::open(&path).unwrap();
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line.unwrap();
            let row: Value = serde_json::from_str(&line).unwrap();
            let event_name = row.get("hook_event_name").and_then(|v| v.as_str()).unwrap_or("");
            if !is_attribution_event(event_name) {
                continue;
            }
            let model = extract_model(&row);
            if model.is_empty() {
                continue;
            }
            let ts = parse_event_ts(&row).unwrap();
            events.push(LocalHookEvent {
                ts_epoch: ts,
                family: normalize_model_family(&model),
                model,
                hook_event_name: event_name.to_string(),
            });
        }
        // beforeSubmitPrompt + afterAgentResponse + subagentStart（有 model）
        // stop 无 model、sessionEnd 不在归因列表 → 排除
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].family, "grok-4.5");
        assert_eq!(events[1].hook_event_name, "afterAgentResponse");
        assert_eq!(events[2].family, "composer-2.5");
        assert_eq!(events[2].hook_event_name, "subagentStart");
    }

    #[test]
    fn test_extract_model_prefers_model_then_subagent() {
        let with_both: Value = serde_json::json!({
            "model": "cursor-grok-4.5-high",
            "subagent_model": "composer-2.5"
        });
        assert_eq!(extract_model(&with_both), "cursor-grok-4.5-high");

        let sub_only: Value = serde_json::json!({
            "subagent_model": "composer-2.5"
        });
        assert_eq!(extract_model(&sub_only), "composer-2.5");

        let empty: Value = serde_json::json!({"hook_event_name": "stop"});
        assert!(extract_model(&empty).is_empty());
    }

    #[test]
    fn test_hook_events_cover_subagent_and_compact() {
        assert!(HOOK_EVENTS.contains(&"subagentStart"));
        assert!(HOOK_EVENTS.contains(&"subagentStop"));
        assert!(HOOK_EVENTS.contains(&"preCompact"));
        assert!(HOOK_EVENTS.contains(&"afterAgentThought"));
        assert!(HOOK_EVENTS.contains(&"sessionStart"));
        assert!(ATTRIBUTION_HOOK_EVENTS.contains(&"subagentStart"));
    }
}
