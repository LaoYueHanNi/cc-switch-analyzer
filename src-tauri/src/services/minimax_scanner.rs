//! MiniMax Code 本地会话用量扫描入库
//!
//! 支持双轨扫描：
//! 1. **主链路 (SQLite)**：直读 `~/.minimax/v2/sqlite/runtime-state.sqlite` 的
//!    `local_runtime_message_rows` 表。提取 `request_duration_ms`（请求总耗时）
//!    与 `thinking_duration_ms`（思考/首字耗时），模型名取自 `context_usage_telemetry.model`。
//!    增量机制基于自增 `id` 游标（记录在 `session_log_sync.last_line_offset`）与文件 mtime。
//! 2. **降级链路 (JSONL)**：若 SQLite 数据库不存在，自动回退扫描
//!    `~/.minimax/v2/sessions/<YYYY>/<MM>/<DD>/<会话目录>/messages.jsonl`，
//!    确保极端环境下的可用性与向后兼容。
//!
//! 解析出的记录统一增量写入应用自有库 `pricing.db::session_request_logs`
//! （source='MiniMax'，provider_id='MiniMax'，主键格式 `MiniMax:{msg_id}`）。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::{metadata_modified_nanos, scan_file_incremental, DshScanResult, ParsedRow};

/// MiniMax 数据源标识（用作 provider_id 与 session_request_logs.source）
pub const MINIMAX_SOURCE: &str = "MiniMax";

/// `~/.minimax` 下会话存储相对路径 (v2 布局)
const SESSIONS_REL: &str = "v2/sessions";

/// `~/.minimax` 下 SQLite 数据库相对路径 (v2 布局)
#[allow(dead_code)]
const SQLITE_REL: &str = "v2/sqlite/runtime-state.sqlite";

/// MiniMax 消息行解析结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMinimaxRow {
    pub request_id: String,
    pub session_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub created_at: i64,
    pub latency: i64,
    pub first_token_latency: i64,
}

/// 规范化模型名（去除 provider 前缀，如 "minimax/MiniMax-M3" -> "MiniMax-M3"，"custom_provider:.../glm-5.3-flash" -> "glm-5.3-flash"）
pub fn normalize_minimax_model(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(pos) = trimmed.rfind('/') {
        trimmed[pos + 1..].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// 从 session extra_data_json 提取并规范化 effectiveModel
fn extract_effective_model(extra_json: Option<&str>) -> Option<String> {
    let s = extra_json?.trim();
    if s.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(s).ok()?;
    let model_str = v.get("effectiveModel").and_then(|m| m.as_str())?;
    let normalized = normalize_minimax_model(model_str);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// 解析 SQLite `local_runtime_message_rows` 中的一行数据。
///
/// 仅当该行是真实 LLM 响应（`usage.request_duration_ms > 0`）时返回 Some。
///
/// 注意：MiniMax Code 在 assistant 行写入时，会把会话的累计 `context_usage`
/// 填进 `usage.input_tokens` / `usage.output_tokens`，导致 greeting、工具中转、
/// 纯文本回复等**非 LLM 调用行**也会带"假 token 消耗"。因此不能仅用 token
/// 字段是否非零来判断有效性，必须用 `usage.request_duration_ms` 是否被填充
/// 来识别真正的请求响应。
pub fn parse_minimax_sqlite_row(
    session_id: &str,
    msg_id: &str,
    created_at_ms: i64,
    data_json: &str,
    session_extra_json: Option<&str>,
) -> Option<ParsedMinimaxRow> {
    let data_json = data_json.trim();
    if data_json.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(data_json).ok()?;
    let usage = v.get("usage")?;

    let num = |key: &str| -> i64 {
        usage
            .get(key)
            .and_then(|x| x.as_i64().or_else(|| x.as_u64().map(|n| n as i64)))
            .unwrap_or(0)
    };

    // 必须存在真实 LLM 响应耗时，否则视为 greeting/工具中转，不入库。
    // 这能避免 MiniMax Code 把累计 context_usage 当成单次请求的 token 消耗。
    let latency = num("request_duration_ms").max(0);
    if latency == 0 {
        return None;
    }

    let input_tokens = num("input_tokens").max(num("input"));
    let output_tokens = num("output_tokens").max(num("output"));
    let cache_read = num("cache_read").max(num("cacheRead"));
    let cache_creation = num("cache_write").max(num("cacheWrite"));

    // 模型提取优先级：
    // 1. context_usage_telemetry.model
    // 2. data.model
    // 3. session.extra_data_json.effectiveModel
    // 4. 兜底为 "MiniMax-M3"（MiniMax Code 默认主力模型，避免使用未定价裸名 "MiniMax"）
    let model = v
        .get("context_usage_telemetry")
        .and_then(|t| t.get("model"))
        .and_then(|m| m.as_str())
        .map(normalize_minimax_model)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            v.get("model")
                .and_then(|m| m.as_str())
                .map(normalize_minimax_model)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| extract_effective_model(session_extra_json))
        .unwrap_or_else(|| "MiniMax-M3".to_string());

    // 思考/首字耗时（毫秒，对应 first_token_latency）
    let first_token_latency = v
        .get("thinking_duration_ms")
        .and_then(|x| x.as_i64().or_else(|| x.as_u64().map(|n| n as i64)))
        .unwrap_or(0)
        .max(0);

    let created_at = (created_at_ms / 1000) as i64;
    let request_id = format!("{}:{}", MINIMAX_SOURCE, msg_id);

    Some(ParsedMinimaxRow {
        request_id,
        session_id: session_id.to_string(),
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at,
        latency,
        first_token_latency,
    })
}

/// 解析单行旧版 MiniMax messages.jsonl（降级链路）。
fn parse_minimax_jsonl_line(line: &str, session_id: Option<&str>) -> Option<ParsedRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let message = v.get("message")?;
    let usage = message.get("usage")?;

    let request_id = v.get("message_id").and_then(|i| i.as_str())?.to_string();
    let model = message
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

    let num = |key: &str| -> i64 {
        usage
            .get(key)
            .and_then(|x| x.as_u64())
            .map(|n| n as i64)
            .unwrap_or(0)
    };
    let input_tokens = num("input");
    let output_tokens = num("output");
    let cache_read = num("cacheRead");
    let cache_creation = num("cacheWrite");

    let ts_ms = message.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0);
    let created_at = (ts_ms / 1000) as i64;

    Some(ParsedRow {
        request_id,
        session_id: session_id.map(|s| s.to_string()),
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at,
        project: String::new(),
        latency: 0,
        first_token_latency: 0,
    })
}

/// 对指定的 MiniMax SQLite 数据库执行增量扫描。
pub fn scan_minimax_sqlite_at(
    app_db: &AppDbService,
    sqlite_path: &Path,
) -> Result<DshScanResult, String> {
    let file_path_str = sqlite_path.to_string_lossy().to_string();
    let metadata = std::fs::metadata(sqlite_path)
        .map_err(|e| format!("读取 MiniMax 数据库元数据失败: {}", e))?;
    let mtime = metadata_modified_nanos(&metadata);
    let prev = app_db.get_session_log_sync_state(MINIMAX_SOURCE, &file_path_str);

    if let Some((prev_mtime, _)) = prev {
        if prev_mtime == mtime {
            return Ok(DshScanResult {
                files_scanned: 0,
                imported: 0,
                skipped: 0,
                errors: 0,
                total_records: app_db.get_session_log_count(MINIMAX_SOURCE).unwrap_or(0),
            });
        }
    }

    let cursor_id = prev.map(|(_, offset)| offset).unwrap_or(0);
    let src = Connection::open_with_flags(sqlite_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("打开 MiniMax SQLite 数据库失败: {}", e))?;

    let has_sessions_table: bool = src
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='local_runtime_sessions'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    let query = if has_sessions_table {
        "SELECT r.id, r.session_id, r.msg_id, r.created_at_ms, r.data_json, s.extra_data_json
         FROM local_runtime_message_rows r
         LEFT JOIN local_runtime_sessions s ON r.session_id = s.session_id
         WHERE r.role = 'assistant' AND r.id > ?
         ORDER BY r.id ASC"
    } else {
        "SELECT r.id, r.session_id, r.msg_id, r.created_at_ms, r.data_json, NULL
         FROM local_runtime_message_rows r
         WHERE r.role = 'assistant' AND r.id > ?
         ORDER BY r.id ASC"
    };

    let mut stmt = src
        .prepare(query)
        .map_err(|e| format!("查询 MiniMax message_rows 失败: {}", e))?;

    let rows = stmt
        .query_map(params![cursor_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|e| format!("读取 MiniMax message_rows 失败: {}", e))?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;
    let mut max_id = cursor_id;

    let tx = app_db
        .conn()
        .unchecked_transaction()
        .map_err(|e| format!("开启事务失败: {}", e))?;

    // 容错修复历史解析中遗留的未定价裸名 'MiniMax' 记录，统一规范化为 'MiniMax-M3'
    let _ = tx.execute(
        "UPDATE session_request_logs SET model = 'MiniMax-M3' WHERE source = 'MiniMax' AND model = 'MiniMax'",
        [],
    );

    for item in rows {
        let (row_id, sess_id, msg_id, created_ms, data_json, session_extra) = match item {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[MINIMAX-SCAN] 读取单行失败: {}", e);
                errors += 1;
                continue;
            }
        };

        if row_id > max_id {
            max_id = row_id;
        }

        let parsed = match parse_minimax_sqlite_row(
            &sess_id,
            &msg_id,
            created_ms,
            &data_json,
            session_extra.as_deref(),
        ) {
            Some(p) => p,
            None => continue,
        };

        match AppDbService::insert_session_log_on_conn(
            &tx,
            MINIMAX_SOURCE,
            &parsed.request_id,
            &parsed.session_id,
            &parsed.model,
            MINIMAX_SOURCE,
            parsed.input_tokens,
            parsed.output_tokens,
            parsed.cache_read,
            parsed.cache_creation,
            parsed.created_at,
            parsed.latency,
            parsed.first_token_latency,
        ) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[MINIMAX-SCAN] 写入记录失败 {}: {}", parsed.request_id, e);
                errors += 1;
            }
        }
    }

    AppDbService::update_session_log_sync_on_conn(
        &tx,
        MINIMAX_SOURCE,
        &file_path_str,
        mtime,
        max_id,
    )?;
    tx.commit()
        .map_err(|e| format!("提交 MiniMax 扫描事务失败: {}", e))?;

    let total = app_db.get_session_log_count(MINIMAX_SOURCE).unwrap_or(0);
    Ok(DshScanResult {
        files_scanned: 1,
        imported,
        skipped,
        errors,
        total_records: total,
    })
}

// ========== 降级扫描：扫描 messages.jsonl 目录 ==========

fn read_session_id_from_manifest(messages_path: &Path) -> Option<String> {
    let manifest_path = messages_path.parent()?.join("manifest.json");
    let text = std::fs::read_to_string(manifest_path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("sessionId").and_then(|s| s.as_str()).map(|s| s.to_string())
}

fn is_minimax_session_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == "messages.jsonl")
        .unwrap_or(false)
}

fn walk_dir_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir_recursive(&path, out);
        } else if is_minimax_session_file(&path) {
            out.push(path);
        }
    }
}

fn walk_minimax_session_files(minimax_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let sessions_dir = minimax_dir.join(SESSIONS_REL);
    if sessions_dir.is_dir() {
        walk_dir_recursive(&sessions_dir, &mut files);
    }
    files.sort();
    files
}

/// 返回 MiniMax v2 sessions 目录下最新 messages.jsonl 的元数据（降级探测）
pub fn latest_session_file_mtime(minimax_dir: &Path) -> Option<std::fs::Metadata> {
    walk_minimax_session_files(minimax_dir)
        .into_iter()
        .filter_map(|p| std::fs::metadata(&p).ok())
        .max_by_key(|m| metadata_modified_nanos(m))
}

/// 获取 MiniMax 数据源的最新元数据（优先 SQLite，无则探测 sessions 文件）。
pub fn minimax_source_mtime() -> Option<std::fs::Metadata> {
    if let Ok(db_path) = crate::utils::get_default_minimax_db_path() {
        if db_path.is_file() {
            return std::fs::metadata(&db_path).ok();
        }
    }
    if let Ok(dir) = crate::utils::get_default_minimax_dir() {
        return latest_session_file_mtime(&dir);
    }
    None
}

/// 对指定 MiniMax 目录执行旧版 JSONL 扫描（降级链路）。
pub fn scan_minimax_files_in(app_db: &AppDbService, minimax_dir: &Path) -> Result<DshScanResult, String> {
    if !minimax_dir.is_dir() {
        let total = app_db.get_session_log_count(MINIMAX_SOURCE).unwrap_or(0);
        return Ok(DshScanResult {
            files_scanned: 0,
            imported: 0,
            skipped: 0,
            errors: 0,
            total_records: total,
        });
    }
    let files = walk_minimax_session_files(minimax_dir);
    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;
    for f in &files {
        let session_id = read_session_id_from_manifest(f);
        match scan_file_incremental(app_db, MINIMAX_SOURCE, f, |line| {
            parse_minimax_jsonl_line(line, session_id.as_deref())
        }) {
            Ok((imp, skp)) => {
                imported += imp;
                skipped += skp;
            }
            Err(e) => {
                log::warn!("[MINIMAX-SYNC] 文件处理失败 {}: {}", f.display(), e);
                errors += 1;
            }
        }
    }
    let total = app_db.get_session_log_count(MINIMAX_SOURCE).unwrap_or(0);
    Ok(DshScanResult {
        files_scanned: files.len() as u32,
        imported,
        skipped,
        errors,
        total_records: total,
    })
}

// ========== 对外统一入口 ==========

/// MiniMax SQLite 数据库是否存在。
pub fn minimax_sqlite_available() -> bool {
    crate::utils::get_default_minimax_db_path()
        .map(|p| p.is_file())
        .unwrap_or(false)
}

/// MiniMax 数据源是否可用（SQLite 存在或 v2 sessions 目录存在）。
pub fn minimax_source_dir_available() -> bool {
    minimax_sqlite_available()
        || crate::utils::get_default_minimax_dir()
            .map(|d| d.join(SESSIONS_REL).is_dir())
            .unwrap_or(false)
}

/// 对指定根目录扫描 MiniMax 数据（优先 SQLite，无则降级 JSONL）。
#[allow(dead_code)]
pub fn scan_minimax_in(app_db: &AppDbService, minimax_dir: &Path) -> Result<DshScanResult, String> {
    let sqlite_path = minimax_dir.join(SQLITE_REL);
    if sqlite_path.is_file() {
        scan_minimax_sqlite_at(app_db, &sqlite_path)
    } else {
        scan_minimax_files_in(app_db, minimax_dir)
    }
}

/// 扫描默认 MiniMax 数据（优先 ~/.minimax/v2/sqlite/runtime-state.sqlite，无则降级）。
pub fn scan_minimax(app_db: &AppDbService) -> Result<DshScanResult, String> {
    if let Ok(db_path) = crate::utils::get_default_minimax_db_path() {
        if db_path.is_file() {
            return scan_minimax_sqlite_at(app_db, &db_path);
        }
    }
    let dir = crate::utils::get_default_minimax_dir()?;
    scan_minimax_files_in(app_db, &dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sqlite_row_with_latency_and_thinking() {
        let data = serde_json::json!({
            "msg_id": "uuid-1",
            "turn_id": "turn-1",
            "role": "assistant",
            "thinking_duration_ms": 3200,
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 200,
                "cache_read": 500,
                "cache_write": 10,
                "request_duration_ms": 8500
            },
            "context_usage_telemetry": {
                "model": "glm-5.3-flash"
            }
        })
        .to_string();

        let row = parse_minimax_sqlite_row("sess-1", "uuid-1", 1789789594320, &data, None)
            .expect("应成功解析");
        assert_eq!(row.request_id, "MiniMax:uuid-1");
        assert_eq!(row.session_id, "sess-1");
        assert_eq!(row.model, "glm-5.3-flash");
        assert_eq!(row.input_tokens, 1000);
        assert_eq!(row.output_tokens, 200);
        assert_eq!(row.cache_read, 500);
        assert_eq!(row.cache_creation, 10);
        assert_eq!(row.latency, 8500);
        assert_eq!(row.first_token_latency, 3200);
        assert_eq!(row.created_at, 1789789594);
    }

    #[test]
    fn test_parse_sqlite_row_fallback_model() {
        let data = serde_json::json!({
            "msg_id": "uuid-2",
            "model": "MiniMax-M3",
            "usage": {
                "input": 50,
                "output": 100,
                "request_duration_ms": 4200
            }
        })
        .to_string();

        let row = parse_minimax_sqlite_row("sess-2", "uuid-2", 1000000, &data, None)
            .expect("应成功解析");
        assert_eq!(row.model, "MiniMax-M3");
        assert_eq!(row.input_tokens, 50);
        assert_eq!(row.output_tokens, 100);
        assert_eq!(row.latency, 4200);
        assert_eq!(row.first_token_latency, 0);
    }

    #[test]
    fn test_parse_sqlite_row_no_latency_ignored() {
        // 没有 usage.request_duration_ms → 视为 greeting/工具中转，不入库
        let data = serde_json::json!({
            "msg_id": "uuid-greeting",
            "usage": { "input": 18718, "output": 289 }
        })
        .to_string();
        assert!(parse_minimax_sqlite_row("sess-g", "uuid-greeting", 1000, &data, None).is_none());
    }

    #[test]
    fn test_parse_sqlite_row_session_extra_model() {
        // data_json 缺失 model，从 session extra_data_json 提取并剥离 provider 前缀
        let data = serde_json::json!({
            "msg_id": "uuid-extra",
            "usage": { "input": 80, "output": 120, "request_duration_ms": 5600 }
        })
        .to_string();

        let extra = serde_json::json!({
            "effectiveModel": "minimax/MiniMax-M3"
        })
        .to_string();

        let row = parse_minimax_sqlite_row(
            "sess-extra",
            "uuid-extra",
            1000000,
            &data,
            Some(&extra),
        )
        .expect("应从 session extra 解析成功");
        assert_eq!(row.model, "MiniMax-M3");

        // 智谱自定义 provider 前缀剥离
        let extra_custom = serde_json::json!({
            "effectiveModel": "custom_provider:zhipu-ai-coding-plan/glm-5.3-flash"
        })
        .to_string();
        let row_custom = parse_minimax_sqlite_row(
            "sess-custom",
            "uuid-custom",
            1000000,
            &data,
            Some(&extra_custom),
        )
        .expect("应成功剥离前缀");
        assert_eq!(row_custom.model, "glm-5.3-flash");

        // 完全没有 model 也没有 extra，兜底为 MiniMax-M3
        let row_default = parse_minimax_sqlite_row("sess-def", "uuid-def", 1000000, &data, None)
            .expect("应兜底解析成功");
        assert_eq!(row_default.model, "MiniMax-M3");
    }

    #[test]
    fn test_parse_sqlite_row_zero_usage_ignored() {
        let data = serde_json::json!({
            "msg_id": "uuid-3",
            "usage": { "input": 0, "output": 0, "cache_read": 0 }
        })
        .to_string();
        assert!(parse_minimax_sqlite_row("sess-3", "uuid-3", 1000, &data, None).is_none());
    }

    #[test]
    fn test_scan_minimax_sqlite_incremental() {
        let app_db = AppDbService::new_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sqlite_path = dir.path().join("runtime-state.sqlite");

        // 构造源数据库
        let src = Connection::open(&sqlite_path).unwrap();
        src.execute_batch(
            "CREATE TABLE local_runtime_message_rows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                msg_id TEXT NOT NULL,
                role TEXT,
                turn_id TEXT,
                created_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL
            );",
        )
        .unwrap();

        let data1 = serde_json::json!({
            "usage": { "input": 10, "output": 20, "request_duration_ms": 1500 },
            "context_usage_telemetry": { "model": "MiniMax-M3" }
        })
        .to_string();

        src.execute(
            "INSERT INTO local_runtime_message_rows (session_id, msg_id, role, turn_id, created_at_ms, data_json)
             VALUES ('s1', 'm1', 'assistant', 't1', 1000000, ?1)",
            params![data1],
        )
        .unwrap();

        // 首次扫描
        let r1 = scan_minimax_sqlite_at(&app_db, &sqlite_path).unwrap();
        assert_eq!(r1.files_scanned, 1);
        assert_eq!(r1.imported, 1);
        assert_eq!(r1.total_records, 1);

        // 验证落库字段包含 latency
        let (lat, ftl): (i64, i64) = app_db
            .conn()
            .query_row(
                "SELECT latency, first_token_latency FROM session_request_logs WHERE request_id = 'MiniMax:m1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(lat, 1500);
        assert_eq!(ftl, 0);

        // 第二次扫描：mtime 未变，跳过
        let r2 = scan_minimax_sqlite_at(&app_db, &sqlite_path).unwrap();
        assert_eq!(r2.imported, 0);

        // 插入第二条记录并触碰 mtime
        let data2 = serde_json::json!({
            "thinking_duration_ms": 800,
            "usage": { "input": 30, "output": 40, "request_duration_ms": 2000 },
            "context_usage_telemetry": { "model": "glm-5.3-flash" }
        })
        .to_string();

        src.execute(
            "INSERT INTO local_runtime_message_rows (session_id, msg_id, role, turn_id, created_at_ms, data_json)
             VALUES ('s1', 'm2', 'assistant', 't1', 2000000, ?1)",
            params![data2],
        )
        .unwrap();

        // 触碰 mtime 到未来以模拟更新
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
        let f = std::fs::File::open(&sqlite_path).unwrap();
        let _ = f.set_modified(future);

        let r3 = scan_minimax_sqlite_at(&app_db, &sqlite_path).unwrap();
        assert_eq!(r3.imported, 1);
        assert_eq!(r3.total_records, 2);

        let (lat2, ftl2): (i64, i64) = app_db
            .conn()
            .query_row(
                "SELECT latency, first_token_latency FROM session_request_logs WHERE request_id = 'MiniMax:m2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(lat2, 2000);
        assert_eq!(ftl2, 800);
    }

    #[test]
    fn test_scan_real_minimax_sqlite() {
        let db_path = match crate::utils::get_default_minimax_db_path() {
            Ok(p) if p.is_file() => p,
            _ => return,
        };
        let app_db = AppDbService::new_in_memory().unwrap();
        let res = scan_minimax_sqlite_at(&app_db, &db_path).unwrap();
        println!("[REAL-SCAN-MINIMAX] {:?}", res);
        assert!(res.imported >= 10, "应导入真实 LLM 响应记录 (过滤了 greeting/工具中转)");

        // 验证带有 latency 的记录数
        let with_latency: i64 = app_db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM session_request_logs WHERE source = 'MiniMax' AND latency > 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        println!("[REAL-SCAN-MINIMAX] 包含 latency 记录数: {}", with_latency);
        assert!(with_latency >= 10, "应有真实 LLM 响应记录包含 latency");

        // 验证没有未定价的裸名 'MiniMax' 记录
        let unpriced_minimax: i64 = app_db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM session_request_logs WHERE source = 'MiniMax' AND model = 'MiniMax'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unpriced_minimax, 0, "不应存在裸名 'MiniMax' 的未定价记录");

        // 验证 scan_minimax_in 接口
        if let Ok(dir) = crate::utils::get_default_minimax_dir() {
            let res2 = scan_minimax_in(&app_db, &dir).unwrap();
            assert_eq!(res2.imported, 0, "mtime 未变第二次扫描应导入 0");
        }
    }
}