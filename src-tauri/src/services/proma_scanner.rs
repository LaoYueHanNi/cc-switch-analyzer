//! Proma 数据源扫描入库
//!
//! 扫描 `~/.proma/agent-sessions/*.jsonl`，增量写入应用自有库
//! `pricing.db::session_request_logs`(source='Proma')，模式与 DSH/MiniMax 一致。
//!
//! 解析兼容两种行格式（自 proma_dir.rs 迁移）：
//! 1. SDK 格式:顶层 `type:"assistant"`,usage 在 `message.usage`(snake_case),时间 `_createdAt` 毫秒
//! 2. 旧 AgentMessage 格式:顶层 `role:"assistant"`,usage 在顶层(camelCase),`createdAt` 毫秒,`durationMs`
//!
//! 同一消息 id 多个流式分片行的「usage 总和取最大」语义由
//! `insert_session_log_on_conn` 的 UPSERT 条件(总和更大才更新)在 SQL 层保证。
//!
//! 项目归属:读取 agent-sessions.json 索引(id → workspaceId)与
//! agent-workspaces.json(workspaceId → 项目名)，按文件名 = 会话 id
//! 关联后写入 sessions 表。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::{metadata_modified_nanos, scan_file_incremental, DshScanResult, ParsedRow};
use crate::utils;

/// Proma 数据源标识(也用作 provider_id 与 session_request_logs.source)
pub const PROMA_SOURCE: &str = "Proma";

/// 一条用量行的解析结果(内部结构,request_key 为去重主键素材)
struct PromaUsage {
    request_key: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read: i64,
    cache_creation: i64,
    created_at: i64,
    latency: i64,
}

/// 加载 workspace 项目名映射:
/// - key = 会话 id(agent-sessions[].id,即会话文件名)
/// - value = 项目名(agent-workspaces[].name)
/// 索引缺失/解析失败返回空映射(仅失去项目归属,不影响用量统计)。
fn load_workspace_names(proma_root: &Path) -> HashMap<String, String> {
    let mut ws_names: HashMap<String, String> = HashMap::new();
    let mut sid_to_ws: HashMap<String, String> = HashMap::new();

    if let Ok(text) = std::fs::read_to_string(proma_root.join("agent-workspaces.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(list) = v.get("workspaces").and_then(Value::as_array) {
                for w in list {
                    if let (Some(id), Some(name)) = (
                        w.get("id").and_then(Value::as_str),
                        w.get("name").and_then(Value::as_str),
                    ) {
                        ws_names.insert(id.to_string(), name.to_string());
                    }
                }
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(proma_root.join("agent-sessions.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(list) = v.get("sessions").and_then(Value::as_array) {
                for s in list {
                    if let (Some(id), Some(ws)) = (
                        s.get("id").and_then(Value::as_str),
                        s.get("workspaceId").and_then(Value::as_str),
                    ) {
                        sid_to_ws.insert(id.to_string(), ws.to_string());
                    }
                }
            }
        }
    }

    // 折叠为 会话id → 项目名
    sid_to_ws
        .into_iter()
        .filter_map(|(sid, ws)| ws_names.get(&ws).cloned().map(|n| (sid, n)))
        .collect()
}

/// 解析单行 JSON 为用量记录。非 assistant 用量行 / 非 JSON 行返回 None。
/// 兼容 SDK 格式(type=assistant)与旧 AgentMessage 格式(role=assistant)。
fn parse_proma_line(line: &str, fallback_seq: usize) -> Option<PromaUsage> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;

    match v.get("type").and_then(Value::as_str) {
        Some("assistant") => parse_sdk_message(&v, fallback_seq),
        Some(_) => None,
        None => {
            if v.get("role").and_then(Value::as_str) == Some("assistant") {
                parse_agent_message(&v, fallback_seq)
            } else {
                None
            }
        }
    }
}

/// SDK 格式：`{type:"assistant", message:{id,model,usage:{...}}, _createdAt, _channelModelId, uuid}`
fn parse_sdk_message(v: &Value, fallback_seq: usize) -> Option<PromaUsage> {
    let msg = v.get("message")?;
    let usage = msg.get("usage")?;
    let num = |key: &str| -> i64 {
        usage.get(key).and_then(Value::as_i64).unwrap_or(0)
    };
    let input_tokens = num("input_tokens");
    let output_tokens = num("output_tokens");
    let cache_read = num("cache_read_input_tokens");
    let cache_creation = num("cache_creation_input_tokens");
    if input_tokens <= 0 && output_tokens <= 0 && cache_read <= 0 && cache_creation <= 0 {
        return None;
    }
    // 模型口径：实际响应模型 message.model，缺失回退渠道配置 _channelModelId
    let model = msg
        .get("model")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            v.get("_channelModelId")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_default();
    let created_ms = v.get("_createdAt").and_then(Value::as_i64)?;
    if created_ms <= 0 {
        return None;
    }
    let request_key = msg
        .get("id")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| v.get("uuid").and_then(Value::as_str).map(String::from))
        .unwrap_or_else(|| format!("sdk-{}-{}", fallback_seq, created_ms));

    Some(PromaUsage {
        request_key,
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at: created_ms / 1000,
        latency: 0,
    })
}

/// 旧 AgentMessage 格式：`{id, role:"assistant", content, createdAt, model, usage:{...camelCase}, durationMs}`
fn parse_agent_message(v: &Value, fallback_seq: usize) -> Option<PromaUsage> {
    let usage = v.get("usage")?;
    let num = |key: &str| -> i64 {
        usage.get(key).and_then(Value::as_i64).unwrap_or(0)
    };
    let input_tokens = num("inputTokens");
    let output_tokens = num("outputTokens");
    let cache_read = num("cacheReadTokens");
    let cache_creation = num("cacheCreationTokens");
    if input_tokens <= 0 && output_tokens <= 0 && cache_read <= 0 && cache_creation <= 0 {
        return None;
    }
    let model = v.get("model").and_then(Value::as_str).unwrap_or_default().to_string();
    let created_ms = v.get("createdAt").and_then(Value::as_i64)?;
    if created_ms <= 0 {
        return None;
    }
    let request_key = v
        .get("id")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| format!("msg-{}-{}", fallback_seq, created_ms));

    Some(PromaUsage {
        request_key,
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at: created_ms / 1000,
        latency: v.get("durationMs").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// 对单个会话文件执行增量扫描(mtime + 行 offset 游标)。
/// 文件名(不含扩展名)即 agent-sessions 索引中的会话 id，用于项目归属。
fn scan_proma_file(
    app_db: &AppDbService,
    file_path: &Path,
    ws_names: &HashMap<String, String>,
) -> Result<(u32, u32), String> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let metadata = std::fs::metadata(file_path)
        .map_err(|e| format!("读取文件元数据失败: {}", e))?;
    let file_modified = metadata_modified_nanos(&metadata);

    let session_id = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    // 该文件对应会话的项目名(索引未收录则为空)
    let project_name = ws_names.get(&session_id).cloned().unwrap_or_default();

    let (last_modified, last_offset) = app_db
        .get_session_log_sync_state(PROMA_SOURCE, &file_path_str)
        .unwrap_or((0, 0));

    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let text = std::fs::read_to_string(file_path)
        .map_err(|e| format!("读取会话文件失败: {}", e))?;

    let conn = app_db.conn();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("开启事务失败: {}", e))?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut line_offset = 0i64;

    for line in text.lines() {
        line_offset += 1;
        if line_offset <= last_offset {
            continue;
        }
        if let Some(u) = parse_proma_line(line, line_offset as usize) {
            let request_id = format!("{}:{}", PROMA_SOURCE, u.request_key);
            match AppDbService::insert_session_log_on_conn(
                &tx,
                PROMA_SOURCE,
                &request_id,
                &session_id,
                &u.model,
                PROMA_SOURCE,
                u.input_tokens,
                u.output_tokens,
                u.cache_read,
                u.cache_creation,
                u.created_at,
                u.latency,
            ) {
                Ok(true) => imported += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    log::warn!("[PROMA-SYNC] 插入失败 ({}): {}", u.request_key, e);
                    skipped += 1;
                }
            }
        }
    }

    // 项目归属入库(索引有该会话且解析出项目名时)
    if !project_name.is_empty() && !session_id.is_empty() {
        let _ = AppDbService::save_session_project_on_conn(&tx, &session_id, &project_name, PROMA_SOURCE);
    }

    AppDbService::update_session_log_sync_on_conn(
        &tx,
        PROMA_SOURCE,
        &file_path_str,
        file_modified,
        line_offset,
    )?;
    tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;

    Ok((imported, skipped))
}

/// 收集 agent-sessions 下全部 jsonl(排序保证增量稳定)
fn walk_proma_session_files(proma_root: &Path) -> Vec<PathBuf> {
    let sessions_dir = proma_root.join("agent-sessions");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&sessions_dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|p| p.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// 返回 agent-sessions 目录下最新会话文件的元数据(供 refresh 的 mtime 检测)
pub fn latest_session_file_mtime(proma_root: &Path) -> Option<std::fs::Metadata> {
    walk_proma_session_files(proma_root)
        .into_iter()
        .filter_map(|p| std::fs::metadata(&p).ok())
        .max_by_key(|m| metadata_modified_nanos(m))
}

/// 对指定 Proma 数据目录执行扫描(可测入口,不依赖 ~/.proma 固定路径)。
pub fn scan_proma_in(app_db: &AppDbService, proma_root: &Path) -> Result<DshScanResult, String> {
    let sessions_dir = proma_root.join("agent-sessions");
    if !sessions_dir.is_dir() {
        let total = app_db.get_session_log_count(PROMA_SOURCE).unwrap_or(0);
        return Ok(DshScanResult {
            files_scanned: 0,
            imported: 0,
            skipped: 0,
            errors: 0,
            total_records: total,
        });
    }
    let ws_names = load_workspace_names(proma_root);
    let files = walk_proma_session_files(proma_root);
    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;
    for f in &files {
        match scan_proma_file(app_db, f, &ws_names) {
            Ok((imp, skp)) => {
                imported += imp;
                skipped += skp;
            }
            Err(e) => {
                log::warn!("[PROMA-SYNC] 文件处理失败 {}: {}", f.display(), e);
                errors += 1;
            }
        }
    }
    let total = app_db.get_session_log_count(PROMA_SOURCE).unwrap_or(0);
    Ok(DshScanResult {
        files_scanned: files.len() as u32,
        imported,
        skipped,
        errors,
        total_records: total,
    })
}

/// 扫描默认 Proma 目录(~/.proma)并增量入库。
pub fn scan_proma(app_db: &AppDbService) -> Result<DshScanResult, String> {
    let dir = utils::get_default_proma_dir()?;
    scan_proma_in(app_db, &dir)
}

/// Proma 数据是否可用(agent-sessions 目录存在)。
pub fn proma_source_dir_available() -> bool {
    utils::get_default_proma_dir()
        .map(|d| d.join("agent-sessions").is_dir())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sdk_assistant_line() {
        let line = serde_json::json!({
            "type": "assistant",
            "message": {
                "id": "msg_1",
                "model": "MiniMax-M3",
                "usage": { "input_tokens": 16299, "output_tokens": 68, "cache_read_input_tokens": 128, "cache_creation_input_tokens": 0 }
            },
            "_createdAt": 1786625853684u64
        })
        .to_string();
        let u = parse_proma_line(&line, 0).expect("应解析出用量");
        assert_eq!(u.request_key, "msg_1");
        assert_eq!(u.model, "MiniMax-M3");
        assert_eq!(u.created_at, 1786625853684 / 1000);
        assert_eq!(u.latency, 0);
    }

    #[test]
    fn test_parse_agent_message_with_duration() {
        let line = serde_json::json!({
            "id": "row-9",
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "usage": { "inputTokens": 15500, "outputTokens": 194, "cacheReadTokens": 0, "cacheCreationTokens": 0 },
            "model": "gpt-5.6",
            "createdAt": 1786456788286u64,
            "durationMs": 13905
        })
        .to_string();
        let u = parse_proma_line(&line, 0).expect("应解析出用量");
        assert_eq!(u.request_key, "row-9");
        assert_eq!(u.latency, 13905);
        assert_eq!(u.created_at, 1786456788286 / 1000);
    }

    #[test]
    fn test_parse_ignores_user_and_result_lines() {
        let user_line = serde_json::json!({
            "type": "user",
            "message": { "content": [{ "type": "text", "text": "hello" }] },
            "_createdAt": 1786456412048u64
        })
        .to_string();
        assert!(parse_proma_line(&user_line, 0).is_none());

        let result_line = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "usage": { "input_tokens": 16378, "output_tokens": 107 }
        })
        .to_string();
        assert!(parse_proma_line(&result_line, 0).is_none());
    }

    #[test]
    fn test_parse_no_usage_returns_none() {
        let line = serde_json::json!({
            "type": "assistant",
            "message": { "id": "msg_x", "content": [{ "type": "text", "text": "no usage here" }] },
            "_createdAt": 1786625853684u64
        })
        .to_string();
        assert!(parse_proma_line(&line, 0).is_none());
    }

    #[test]
    fn test_parse_invalid_json_returns_none() {
        assert!(parse_proma_line("not json", 0).is_none());
        assert!(parse_proma_line("", 0).is_none());
    }
}
