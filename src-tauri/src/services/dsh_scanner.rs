//! DSH(DeepSeek Harness)本地会话用量扫描入库
//!
//! 支持两种数据来源(settings 表 `dsh_use_plugin` 切换,缺省会话扫描):
//! - **会话扫描**:扫描 `~/.dsh/sessions/<编码项目目录>/session-<uuid>/session.jsonl.zstd`,
//!   zstd 解压后提取 `assistant/message` 事件的 usage。
//! - **插件数据**:扫描插件 dsh-token-usage 写入的 `~/.dsh/token-usage/usage-*.jsonl`
//!   (见 [`crate::services::dsh_plugin_scanner`])。
//!
//! 两种来源解析出的行统一为 [`ParsedRow`],经 [`scan_file_incremental`] 增量写入
//! 应用自有库 `pricing.db::session_request_logs`(通用表,source='dsh',
//! request_id = "dsh:" + 消息 id,两种来源请求级去重)。
//!
//! 增量机制参考 cc-switch 的 session_usage.rs:
//! - session_log_sync 表记录 (file_path, source) → (mtime 纳秒, 行 offset)
//! - 文件 mtime 未变则跳过整文件
//! - mtime 变了则全量解压(zstd 无法流式 seek),只处理 last_offset 之后的新行,
//!   INSERT OR IGNORE 主键去重,推进 offset
//!
//! 数据格式见 DSH_INTEGRATION_HANDOFF.md §1(已验证):
//! - assistant/message 的 data.usage(inputTokens/outputTokens/cacheReadTokens/cacheWriteTokens)
//! - data.message.source.model(缺失 fallback data.message.model → "unknown")
//! - data.message.id(去重 key)
//! - time 毫秒 → /1000 转秒
//! - assistant/chunk 的 usage 是快照,忽略(避免重复计数)

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::services::app_db::AppDbService;

/// DSH 数据源标识(也用作 provider_id 与 session_request_logs.source)
pub const DSH_SOURCE: &str = "DSH";

/// 数据来源模式设置 key(settings 表):"1" 使用插件数据,其余走会话扫描
pub const SETTING_DSH_USE_PLUGIN: &str = "dsh_use_plugin";

/// 插件数据目录自定义设置 key(settings 表):非空时覆盖默认解析
/// (插件新版支持自定义数据目录,用户在此显式指定其路径)
pub const SETTING_DSH_PLUGIN_DATA_DIR: &str = "dsh_plugin_data_dir";

/// 一条已解析的用量行(会话事件解析 / 插件记录解析的统一结构)
#[derive(Debug, Clone)]
pub struct ParsedRow {
    /// 原始请求标识(会话事件:message.id;插件记录:requestId)
    pub request_id: String,
    pub session_id: Option<String>,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub created_at: i64,
}

/// 扫描结果(序列化给前端)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshScanResult {
    pub files_scanned: u32,
    pub imported: u32,
    pub skipped: u32,
    pub errors: u32,
    pub total_records: i64,
}

/// 读取 DSH 日志文件(会话/插件 JSONL 共用)并解压为文本。
///
/// 兼容 zstd 压缩(magic bytes `28 b5 2f fd`)与明文 JSONL 两种格式。
/// 解压/解码失败返回 Err(由调用方捕获后跳过该文件)。
fn read_log_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
    let is_zstd = bytes.len() >= 4 && bytes[0..4] == [0x28, 0xb5, 0x2f, 0xfd];
    if is_zstd {
        let decoded = zstd::decode_all(bytes.as_slice())
            .map_err(|e| format!("zstd 解压失败: {}", e))?;
        String::from_utf8(decoded).map_err(|e| format!("UTF-8 解码失败: {}", e))
    } else {
        String::from_utf8(bytes).map_err(|e| format!("UTF-8 解码失败: {}", e))
    }
}

/// 解析一行 DSH 会话事件。
///
/// - `session` 事件:记录 session id 到 `current_session_id`,返回 None
/// - `assistant/message` 事件:提取 usage + model,返回 Some(ParsedRow)
/// - 其他事件(含 assistant/chunk 快照):返回 None
///
/// 解析失败(非 JSON 等)返回 None,容忍不完整行。
fn parse_dsh_line(line: &str, current_session_id: &mut Option<String>) -> Option<ParsedRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    if event_type == "session" {
        if let Some(id) = v
            .get("data")
            .and_then(|d| d.get("id"))
            .and_then(|i| i.as_str())
        {
            *current_session_id = Some(id.to_string());
        }
        return None;
    }

    if event_type != "assistant/message" {
        return None;
    }

    let data = v.get("data")?;
    let message = data.get("message")?;
    let usage = data.get("usage")?;

    let request_id = message.get("id").and_then(|i| i.as_str())?.to_string();

    // model: message.source.model → message.model → "unknown"
    let model = message
        .get("source")
        .and_then(|s| s.get("model"))
        .and_then(|m| m.as_str())
        .or_else(|| message.get("model").and_then(|m| m.as_str()))
        .unwrap_or("unknown")
        .to_string();

    let num = |key: &str| -> i64 {
        usage
            .get(key)
            .and_then(|x| x.as_u64())
            .map(|n| n as i64)
            .unwrap_or(0)
    };
    let input_tokens = num("inputTokens");
    let output_tokens = num("outputTokens");
    let cache_read = num("cacheReadTokens");
    let cache_creation = num("cacheWriteTokens");

    // time 毫秒 → 秒
    let time_ms = v.get("time").and_then(|t| t.as_u64()).unwrap_or(0);
    let created_at = (time_ms / 1000) as i64;

    Some(ParsedRow {
        request_id,
        session_id: current_session_id.clone(),
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at,
    })
}

/// 返回文件 mtime 的纳秒时间戳(照搬 cc-switch metadata_modified_nanos)
pub(crate) fn metadata_modified_nanos(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn is_dsh_session_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    name.ends_with(".zstd") || name.ends_with(".jsonl")
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
        } else if is_dsh_session_file(&path) {
            out.push(path);
        }
    }
}

/// 收集 `~/.dsh/sessions/` 下所有 .zstd/.jsonl 会话文件(排序保证增量稳定)
fn walk_dsh_session_files(dsh_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let sessions_dir = dsh_dir.join("sessions");
    if sessions_dir.is_dir() {
        walk_dir_recursive(&sessions_dir, &mut files);
    }
    files.sort();
    files
}

/// 返回 DSH sessions 目录下最新会话文件的元数据(供 refresh 的 mtime 检测)
pub fn latest_session_file_mtime(dsh_dir: &Path) -> Option<std::fs::Metadata> {
    walk_dsh_session_files(dsh_dir)
        .into_iter()
        .filter_map(|p| std::fs::metadata(&p).ok())
        .max_by_key(|m| metadata_modified_nanos(m))
}

/// 对单个 JSONL 日志文件执行增量扫描(会话事件 / 插件记录共用)。
///
/// 文件 mtime 未变 → 整文件跳过返回 (0,0)。
/// 否则全量读取,从 last_offset+1 行解析(parse 返回 None 的行跳过),
/// 事务内 INSERT OR IGNORE + 推进 offset。
pub fn scan_file_incremental(
    app_db: &AppDbService,
    source: &str,
    file_path: &Path,
    mut parse: impl FnMut(&str) -> Option<ParsedRow>,
) -> Result<(u32, u32), String> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let metadata = std::fs::metadata(file_path)
        .map_err(|e| format!("读取文件元数据失败: {}", e))?;
    let file_modified = metadata_modified_nanos(&metadata);

    let (last_modified, last_offset) = app_db
        .get_session_log_sync_state(source, &file_path_str)
        .unwrap_or((0, 0));

    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let text = read_log_file(file_path)?;

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
        if let Some(msg) = parse(line) {
            // 任一计费维度 > 0 即导入(照搬 cc-switch has_billable_token)
            let has_billable = msg.input_tokens > 0
                || msg.output_tokens > 0
                || msg.cache_read > 0
                || msg.cache_creation > 0;
            if !has_billable {
                continue;
            }
            let request_id = format!("{}:{}", source, msg.request_id);
            match AppDbService::insert_session_log_on_conn(
                &tx,
                source,
                &request_id,
                msg.session_id.as_deref().unwrap_or(""),
                &msg.model,
                source,
                msg.input_tokens,
                msg.output_tokens,
                msg.cache_read,
                msg.cache_creation,
                msg.created_at,
            ) {
                Ok(true) => imported += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    log::warn!("[DSH-SYNC] 插入失败 ({}): {}", msg.request_id, e);
                    skipped += 1;
                }
            }
        }
    }

    AppDbService::update_session_log_sync_on_conn(
        &tx,
        source,
        &file_path_str,
        file_modified,
        line_offset,
    )?;
    tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;

    Ok((imported, skipped))
}

/// 对指定目录执行会话文件扫描(可测入口,不依赖 ~/.dsh 固定路径)。
pub fn scan_dsh_in(app_db: &AppDbService, dsh_dir: &Path) -> Result<DshScanResult, String> {
    if !dsh_dir.is_dir() {
        let total = app_db.get_session_log_count(DSH_SOURCE).unwrap_or(0);
        return Ok(DshScanResult {
            files_scanned: 0,
            imported: 0,
            skipped: 0,
            errors: 0,
            total_records: total,
        });
    }
    let files = walk_dsh_session_files(dsh_dir);
    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;
    for f in &files {
        let mut current_session_id: Option<String> = None;
        match scan_file_incremental(app_db, DSH_SOURCE, f, |line| {
            parse_dsh_line(line, &mut current_session_id)
        }) {
            Ok((imp, skp)) => {
                imported += imp;
                skipped += skp;
            }
            Err(e) => {
                log::warn!("[DSH-SYNC] 文件处理失败 {}: {}", f.display(), e);
                errors += 1;
            }
        }
    }
    let total = app_db.get_session_log_count(DSH_SOURCE).unwrap_or(0);
    Ok(DshScanResult {
        files_scanned: files.len() as u32,
        imported,
        skipped,
        errors,
        total_records: total,
    })
}

/// 扫描默认 DSH 目录(~/.dsh)并增量入库(会话扫描模式)。
pub fn scan_dsh(app_db: &AppDbService) -> Result<DshScanResult, String> {
    let dir = crate::utils::get_default_dsh_dir()?;
    scan_dsh_in(app_db, &dir)
}

// ========== 数据来源模式 ==========

/// 当前 DSH 数据来源是否为插件数据(settings 表 `dsh_use_plugin` = "1")。
pub fn dsh_use_plugin(app_db: &AppDbService) -> bool {
    app_db.get_setting(SETTING_DSH_USE_PLUGIN).as_deref() == Some("1")
}

/// 持久化 DSH 数据来源模式(开启插件数据 / 会话扫描)。
pub fn set_dsh_use_plugin(app_db: &AppDbService, use_plugin: bool) -> Result<(), String> {
    app_db.set_setting(SETTING_DSH_USE_PLUGIN, if use_plugin { "1" } else { "0" })
}

/// 按当前模式扫描 DSH 数据并增量入库(插件数据 / 会话扫描)。
pub fn scan_dsh_by_mode(app_db: &AppDbService) -> Result<DshScanResult, String> {
    if dsh_use_plugin(app_db) {
        let dir = resolve_dsh_plugin_dir(app_db)?;
        crate::services::dsh_plugin_scanner::scan_plugin_in(app_db, &dir)
    } else {
        scan_dsh(app_db)
    }
}

/// 当前模式下 DSH 数据目录是否存在(用于数据源注册判断)。
pub fn dsh_source_dir_available(app_db: &AppDbService) -> bool {
    if dsh_use_plugin(app_db) {
        resolve_dsh_plugin_dir(app_db)
            .map(|d| d.is_dir())
            .unwrap_or(false)
    } else {
        crate::utils::get_default_dsh_dir()
            .map(|d| d.is_dir())
            .unwrap_or(false)
    }
}

/// 解析插件数据目录:settings 自定义目录非空时完全覆盖默认解析,
/// 否则回落 [`crate::utils::get_default_dsh_plugin_dir`]
/// (`$DSH_HOME/token-usage` 或 `~/.dsh/token-usage`)。
///
/// 自定义目录可能已不存在(如被删除),此时仍返回该路径,
/// 由调用方按 `plugin_installed` 等存在性状态呈现。
pub fn resolve_dsh_plugin_dir(app_db: &AppDbService) -> Result<PathBuf, String> {
    if let Some(custom) = app_db.get_setting(SETTING_DSH_PLUGIN_DATA_DIR) {
        let custom = custom.trim().to_string();
        if !custom.is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }
    crate::utils::get_default_dsh_plugin_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_msg(id: &str, model: &str, time_ms: u64, inp: u64, out: u64, cr: u64, cw: u64) -> String {
        serde_json::json!({
            "type": "assistant/message",
            "seq": 1,
            "time": time_ms,
            "data": {
                "turn": 1, "step": 1,
                "message": {
                    "role": "assistant",
                    "content": [],
                    "id": id,
                    "source": { "kind": "model", "provider": "opencode-go", "model": model }
                },
                "usage": {
                    "inputTokens": inp,
                    "outputTokens": out,
                    "cacheReadTokens": cr,
                    "cacheWriteTokens": cw
                }
            }
        })
        .to_string()
    }

    #[test]
    fn test_parse_assistant_message() {
        let line = assistant_msg("m1", "deepseek-v4-flash", 1786629109285, 14175, 220, 195072, 0);
        let mut sid = None;
        let m = parse_dsh_line(&line, &mut sid).expect("应解析出消息");
        assert_eq!(m.request_id, "m1");
        assert_eq!(m.model, "deepseek-v4-flash");
        assert_eq!(m.input_tokens, 14175);
        assert_eq!(m.output_tokens, 220);
        assert_eq!(m.cache_read, 195072);
        assert_eq!(m.cache_creation, 0);
        // 毫秒 → 秒
        assert_eq!(m.created_at, 1786629109285 / 1000);
    }

    #[test]
    fn test_parse_model_fallback_to_message_model() {
        // 无 source.model,fallback 到 message.model
        let line = serde_json::json!({
            "type": "assistant/message",
            "time": 1000u64,
            "data": {
                "message": { "id": "m2", "model": "fallback-model" },
                "usage": { "inputTokens": 10, "outputTokens": 0, "cacheReadTokens": 0, "cacheWriteTokens": 0 }
            }
        })
        .to_string();
        let m = parse_dsh_line(&line, &mut None).expect("应解析出消息");
        assert_eq!(m.model, "fallback-model");
    }

    #[test]
    fn test_parse_ignores_chunk() {
        let line = serde_json::json!({
            "type": "assistant/chunk",
            "time": 1000u64,
            "data": { "chunk": { "type": "usage", "usage": { "inputTokens": 999 } } }
        })
        .to_string();
        assert!(parse_dsh_line(&line, &mut None).is_none());
    }

    #[test]
    fn test_parse_session_event_records_id() {
        let line = serde_json::json!({
            "type": "session",
            "time": 1000u64,
            "data": { "id": "sess-abc", "createdAt": 1000, "cwd": "/tmp" }
        })
        .to_string();
        let mut sid = None;
        assert!(parse_dsh_line(&line, &mut sid).is_none());
        assert_eq!(sid.as_deref(), Some("sess-abc"));

        // 后续 assistant/message 应带上该 session_id
        let msg = assistant_msg("m3", "m", 1000, 1, 0, 0, 0);
        let m = parse_dsh_line(&msg, &mut sid).expect("应解析出消息");
        assert_eq!(m.session_id.as_deref(), Some("sess-abc"));
    }

    #[test]
    fn test_parse_invalid_json_returns_none() {
        assert!(parse_dsh_line("not json", &mut None).is_none());
        assert!(parse_dsh_line("", &mut None).is_none());
        assert!(parse_dsh_line("   ", &mut None).is_none());
    }

    #[test]
    fn test_read_log_file_zstd_and_plain() {
        let dir = tempfile::tempdir().unwrap();
        let original = "line1\nline2\n";
        // zstd 压缩
        let zst_path = dir.path().join("session.jsonl.zstd");
        let compressed = zstd::encode_all(original.as_bytes(), 3).unwrap();
        std::fs::write(&zst_path, &compressed).unwrap();
        assert_eq!(read_log_file(&zst_path).unwrap(), original);
        // 明文
        let plain_path = dir.path().join("session.jsonl");
        std::fs::write(&plain_path, original).unwrap();
        assert_eq!(read_log_file(&plain_path).unwrap(), original);
    }

    #[test]
    fn test_walk_dsh_session_files() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions").join("--proj--").join("session-x");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("session.jsonl.zstd"), b"x").unwrap();
        std::fs::write(sessions.join("notes.txt"), b"x").unwrap();
        let files = walk_dsh_session_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().ends_with("session.jsonl.zstd"));
    }

    /// 把文件 mtime 设到未来,确保 > 已记录的旧 mtime 以触发重扫
    fn set_mtime_future(path: &Path) {
        use std::time::{Duration, SystemTime};
        let future = SystemTime::now() + Duration::from_secs(120);
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.set_modified(future);
        }
    }

    #[test]
    fn test_scan_dsh_incremental() {
        let db = AppDbService::new_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions").join("--p--").join("session-1");
        std::fs::create_dir_all(&sessions).unwrap();
        let zst = sessions.join("session.jsonl.zstd");

        let content = format!(
            "{}\n{}\n",
            serde_json::json!({ "type": "session", "time": 1000u64, "data": { "id": "s1" } }),
            assistant_msg("a1", "deepseek-v4-flash", 2_000_000, 100, 10, 50, 0)
        );
        let compressed = zstd::encode_all(content.as_bytes(), 3).unwrap();
        std::fs::write(&zst, &compressed).unwrap();

        // 第一次扫描:导入 1 条
        let r1 = scan_dsh_in(&db, dir.path()).unwrap();
        assert_eq!(r1.imported, 1);
        assert_eq!(r1.total_records, 1);

        // 第二次扫描:mtime 未变,跳过
        let r2 = scan_dsh_in(&db, dir.path()).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.total_records, 1);

        // 追加一行 + 触碰 mtime → 再扫导入新行,总数 2,不重复
        let content2 = format!(
            "{}\n{}\n",
            content,
            assistant_msg("a2", "deepseek-v4-flash", 3_000_000, 200, 20, 0, 0)
        );
        let compressed2 = zstd::encode_all(content2.as_bytes(), 3).unwrap();
        std::fs::write(&zst, &compressed2).unwrap();
        // 强制更新 mtime(确保 > 旧值)
        set_mtime_future(&zst);

        let r3 = scan_dsh_in(&db, dir.path()).unwrap();
        assert_eq!(r3.imported, 1);
        assert_eq!(r3.total_records, 2);
    }

    #[test]
    fn test_mode_setting_roundtrip() {
        let db = AppDbService::new_in_memory().unwrap();
        assert!(!dsh_use_plugin(&db), "缺省应为会话扫描模式");
        set_dsh_use_plugin(&db, true).unwrap();
        assert!(dsh_use_plugin(&db));
        set_dsh_use_plugin(&db, false).unwrap();
        assert!(!dsh_use_plugin(&db));
    }

    #[test]
    fn test_resolve_plugin_dir_custom_overrides_default() {
        let db = AppDbService::new_in_memory().unwrap();
        let default = crate::utils::get_default_dsh_plugin_dir().unwrap();

        // 未设置 → 默认解析
        assert_eq!(resolve_dsh_plugin_dir(&db).unwrap(), default);

        // 非空自定义目录 → 完全覆盖默认(即使目录不存在也直接用)
        let custom = if cfg!(windows) { r"C:\my-usage" } else { "/tmp/my-usage" };
        db.set_setting(SETTING_DSH_PLUGIN_DATA_DIR, custom).unwrap();
        assert_eq!(resolve_dsh_plugin_dir(&db).unwrap(), PathBuf::from(custom));

        // 空白串视为未设置 → 回落默认
        db.set_setting(SETTING_DSH_PLUGIN_DATA_DIR, "  ").unwrap();
        assert_eq!(resolve_dsh_plugin_dir(&db).unwrap(), default);
    }

    /// 设置自定义目录后,插件模式扫描按自定义目录执行;清空后回落默认目录(无数据可扫)
    #[test]
    fn test_scan_by_mode_uses_custom_plugin_dir() {
        let db = AppDbService::new_in_memory().unwrap();
        set_dsh_use_plugin(&db, true).unwrap();

        // 自定义目录:一条插件记录
        let custom_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            custom_dir.path().join("usage-2026-08-20.jsonl"),
            format!(
                "{}\n",
                serde_json::json!({
                    "requestId": "c1", "time": 2_000_000u64, "sessionId": "s1",
                    "model": "m", "usage": {"inputTokens": 10, "outputTokens": 5}
                })
            ),
        )
        .unwrap();
        db.set_setting(SETTING_DSH_PLUGIN_DATA_DIR, custom_dir.path().to_str().unwrap())
            .unwrap();

        assert!(dsh_source_dir_available(&db));
        let r = scan_dsh_by_mode(&db).unwrap();
        assert_eq!(r.files_scanned, 1);
        assert_eq!(r.imported, 1);

        // 清空自定义 → 回落默认目录;自定义目录已入库的记录保留
        // (默认目录是否存在因机器而异,不断言 imported == 0)
        db.set_setting(SETTING_DSH_PLUGIN_DATA_DIR, "").unwrap();
        assert_eq!(
            resolve_dsh_plugin_dir(&db).unwrap(),
            crate::utils::get_default_dsh_plugin_dir().unwrap()
        );
        let r2 = scan_dsh_by_mode(&db).unwrap();
        assert!(r2.total_records >= 1);
    }

    /// 真实数据验证(扫描 ~/.dsh 到内存库,只读不改源文件)
    #[test]
    #[ignore]
    fn test_scan_real_dsh() {
        let dir = match crate::utils::get_default_dsh_dir() {
            Ok(d) if d.is_dir() => d,
            _ => {
                eprintln!("跳过:~/.dsh 不存在");
                return;
            }
        };
        let db = AppDbService::new_in_memory().unwrap();
        let result = scan_dsh_in(&db, &dir).unwrap();
        println!("[REAL-DSH] {:?}", result);
        assert!(result.files_scanned > 0, "应扫描到会话文件");
        assert!(result.total_records > 0, "应导入记录");
        // 验证模型字段有值
        let has_model: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) > 0 FROM session_request_logs WHERE source='DSH' AND model <> ''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(has_model, "应至少有一条带 model 的记录");
        // 打印模型分布
        let mut stmt = db
            .conn()
            .prepare("SELECT model, COUNT(*), SUM(input_tokens), SUM(output_tokens) FROM session_request_logs WHERE source='DSH' GROUP BY model ORDER BY 2 DESC")
            .unwrap();
        let rows = stmt.query_map([], |r| {
            Ok(format!(
                "  model={} requests={} input={} output={}",
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?
            ))
        }).unwrap();
        println!("[REAL-DSH] 模型分布:");
        for r in rows {
            println!("{}", r.unwrap());
        }
    }
}
