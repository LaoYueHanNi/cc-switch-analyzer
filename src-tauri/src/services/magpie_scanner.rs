//! Magpie 数据源（magpie 网关请求账本）本地用量扫描入库
//!
//! 只接入 magpie **网关自己**记录的请求账本 `usage.jsonl`：一行是一次网关转发 /
//! 路由的真实调用（时间、模型、四类 Token、耗时、首字、状态、会话等）。
//! 不接入 magpie 从 agent 会话日志扫描出的用量（`routing/` 是路由轨迹调试数据，
//! `usage-others/` 是其他电脑同步来的账本），也不接入客户端通过
//! `POST /v1/magpie/usage/requests` 上报、实际未经过网关的 `source = "reported"` 记录。
//!
//! 数据源位置（`appdir.Config()` 的解析规则，magpie 源码 `internal/appdir`）：
//! - `$XDG_CONFIG_HOME/magpie/usage.jsonl`（仅当 `XDG_CONFIG_HOME` 为绝对路径）
//! - 默认 `~/.config/magpie/usage.jsonl`（Windows 同样在 `%USERPROFILE%\.config\magpie`，
//!   不是 `%APPDATA%`）
//! - portable 模式的 `<magpie.exe 旁>/data/usage.jsonl` 无法自动定位可执行文件，
//!   由设置页自定义目录（settings 表 `magpie_data_dir`）覆盖
//!
//! 解析后的数据写入应用自有库 `pricing.db::session_request_logs`，`source = "Magpie"`。
//! 不写 `sessions` / `session_titles`，不提供会话管理或项目归属。
//!
//! 增量与去重：
//! - `usage.jsonl` 是 append-only 文件（magpie 只追加、不轮转、不删除），因此以
//!   「文件 mtime + 已读字节偏移」为增量游标；`session_log_sync.last_line_offset`
//!   在本源明确用作**字节偏移**（不是行号）。
//! - 去重键取整行内容的稳定哈希（FNV-1a 64 位 + 行字节长度），写入
//!   `session_request_logs.request_id`。文件被重写、游标丢失或重复扫描都不会双计。
//! - 尾部尚未写完的半行不解析、不推进游标，留待下一轮；文件变短（截断 / 替换）
//!   时重置游标从头读，由主键去重兜底。

use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::{metadata_modified_nanos, DshScanResult, ParsedRow};

/// Magpie 数据源标识（同时也用作 provider_id 与 session_request_logs.source）
pub const MAGPIE_SOURCE: &str = "Magpie";

/// 自定义 magpie 配置目录（settings 表）；空串表示未设置、按默认规则解析
pub const SETTING_MAGPIE_DATA_DIR: &str = "magpie_data_dir";

/// magpie 网关账本文件名（对应 magpie `usage.Path()`）
const USAGE_FILE_NAME: &str = "usage.jsonl";

/// magpie 记录里表示「客户端上报、未经网关」的 source 值（usage.SourceReported）
const SOURCE_REPORTED: &str = "reported";

/// 稳定哈希：FNV-1a 64 位。刻意不依赖标准库 DefaultHasher（其算法不承诺跨版本
/// 稳定），保证应用升级后同一行的去重键不变。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// 去掉行尾换行与首尾 ASCII 空白（哈希与长度都基于清理后的内容，保证稳定）
fn trimmed_line_bytes(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && matches!(line[end - 1], b'\n' | b'\r' | b' ' | b'\t') {
        end -= 1;
    }
    let mut start = 0;
    while start < end && matches!(line[start], b' ' | b'\t') {
        start += 1;
    }
    &line[start..end]
}

/// 一行的去重键（不含 `Magpie:` 前缀）：内容哈希 + 清理后长度。
pub fn request_id_for_line(line: &[u8]) -> String {
    let content = trimmed_line_bytes(line);
    format!("{:016x}:{:x}", fnv1a64(content), content.len())
}

/// 读取 JSON 数字为整数（magpie 写整型；容忍浮点写法）
fn json_int(value: Option<&Value>) -> i64 {
    match value {
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_f64().map(|f| f.round() as i64))
            .unwrap_or(0),
        None => 0,
    }
}

/// RFC3339（Go time.Time 的 RFC3339Nano）转 Unix 秒
fn parse_time_seconds(raw: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

/// 解析一行 magpie 网关账本。
///
/// 返回 None 的情况：非 JSON / 缺 `t` / `source = "reported"` / 四类 Token 全 0
/// （本地拒绝、无计费的失败请求）/ 显式 `rejected`。`handed_on` 为 true 的交接行
/// 若携带 Token 则保留：token 口径与 magpie 汇总一致（其 `Calls` 不计交接行，
/// 本应用的请求数按记录数统计，会略高于 magpie 的 Calls）。
pub fn parse_magpie_line(line: &[u8]) -> Option<ParsedRow> {
    let content = trimmed_line_bytes(line);
    if content.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(content).ok()?;
    let value: Value = serde_json::from_str(text).ok()?;

    if value.get("source").and_then(|v| v.as_str()) == Some(SOURCE_REPORTED) {
        return None;
    }
    if value.get("rejected").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }

    let created_at = value
        .get("t")
        .and_then(|v| v.as_str())
        .and_then(parse_time_seconds)?;

    let input_tokens = json_int(value.get("in"));
    let output_tokens = json_int(value.get("out"));
    let cache_read = json_int(value.get("cache_read"));
    let cache_creation = json_int(value.get("cache_write"));
    if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_creation == 0 {
        return None;
    }

    // model 是供应商实际模型 id；缺失时按 served → req 回退，避免丢计费口径
    let model = ["model", "served", "req"]
        .iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());

    let request_id = request_id_for_line(line);
    // 会话 id 用于实时页与跨源去重指纹；magpie 记录可能没有会话头，
    // 此时用唯一值兜底，避免同 model/token 的不同请求被去重合并。
    let session_id = value
        .get("session")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("magpie:{request_id}"));

    Some(ParsedRow {
        request_id,
        session_id: Some(session_id),
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at,
        project: String::new(),
        latency: json_int(value.get("ms")),
        first_token_latency: json_int(value.get("ttft_ms")),
    })
}

/// 默认的 magpie 配置目录（不读设置）：XDG 绝对路径优先，否则 `~/.config/magpie`。
pub fn default_magpie_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let trimmed = xdg.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.is_absolute() {
                return Some(path.join("magpie"));
            }
        }
    }
    dirs::home_dir().map(|home| home.join(".config").join("magpie"))
}

/// 实际生效的 magpie 配置目录：设置里的自定义目录优先，否则默认规则。
pub fn resolve_magpie_dir(app_db: &AppDbService) -> Option<PathBuf> {
    let custom = app_db
        .get_setting(SETTING_MAGPIE_DATA_DIR)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match custom {
        Some(dir) => Some(PathBuf::from(dir)),
        None => default_magpie_dir(),
    }
}

/// 实际生效的 `usage.jsonl` 路径（不要求存在）
pub fn magpie_usage_path(app_db: &AppDbService) -> Option<PathBuf> {
    resolve_magpie_dir(app_db).map(|dir| dir.join(USAGE_FILE_NAME))
}

/// 数据源是否可用：配置目录存在即可注册。magpie 已安装但尚无请求时目录存在、
/// 账本尚未创建，扫描返回 0 条即可，避免用户首次请求后还要重启才发现数据源。
pub fn magpie_source_available(app_db: &AppDbService) -> bool {
    resolve_magpie_dir(app_db)
        .map(|dir| dir.is_dir())
        .unwrap_or(false)
}

/// 设置页展示的 Magpie 数据源状态
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MagpieSettings {
    /// 实际生效的配置目录
    pub data_dir: Option<String>,
    /// 用户自定义目录（未设置为 None）
    pub custom_data_dir: Option<String>,
    /// 实际生效的 usage.jsonl 路径（可能尚不存在）
    pub usage_file: Option<String>,
    /// usage.jsonl 是否存在
    pub usage_file_exists: bool,
    /// 已入库的 Magpie 记录数
    pub total_records: i64,
}

/// 读取当前 Magpie 数据源设置与入库量
pub fn collect_magpie_settings(app_db: &AppDbService) -> MagpieSettings {
    let custom_data_dir = app_db
        .get_setting(SETTING_MAGPIE_DATA_DIR)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let data_dir = resolve_magpie_dir(app_db);
    let usage_file = data_dir.as_ref().map(|dir| dir.join(USAGE_FILE_NAME));
    MagpieSettings {
        data_dir: data_dir.map(|p| p.to_string_lossy().to_string()),
        custom_data_dir,
        usage_file: usage_file
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        usage_file_exists: usage_file.as_ref().map(|p| p.is_file()).unwrap_or(false),
        total_records: app_db.get_session_log_count(MAGPIE_SOURCE).unwrap_or(0),
    }
}

/// 写入自定义 magpie 配置目录（已规范化）；空串表示恢复默认。
pub fn set_magpie_data_dir(app_db: &AppDbService, normalized: &str) -> Result<(), String> {
    if normalized.is_empty() {
        log::info!("[Magpie] 配置目录恢复默认");
    } else {
        log::info!("[Magpie] 配置目录自定义: {}", normalized);
    }
    app_db.set_setting(SETTING_MAGPIE_DATA_DIR, normalized)
}

/// 单文件增量扫描：从已读字节偏移继续，只导入新增的完整行。
fn scan_magpie_file_incremental(
    app_db: &AppDbService,
    path: &Path,
) -> Result<(u32, u32), String> {
    let file_path_str = path.to_string_lossy().to_string();
    let metadata = std::fs::metadata(path).map_err(|e| format!("读取文件元数据失败: {}", e))?;
    let file_modified = metadata_modified_nanos(&metadata);
    let file_len = metadata.len();

    let (last_modified, last_offset) = app_db
        .get_session_log_sync_state(MAGPIE_SOURCE, &file_path_str)
        .unwrap_or((0, 0));
    // 文件变短：被截断、替换或换成了别的文件，游标失效，从头读（主键去重兜底）
    let mut byte_offset = if last_offset < 0 { 0 } else { last_offset as u64 };
    if byte_offset > file_len {
        log::info!(
            "[Magpie-SCAN] usage.jsonl 变小（{} -> {} 字节），重置游标重读",
            byte_offset,
            file_len
        );
        byte_offset = 0;
    }
    if file_modified <= last_modified && byte_offset == file_len {
        return Ok((0, 0));
    }

    let file = std::fs::File::open(path).map_err(|e| format!("读取 usage.jsonl 失败: {}", e))?;
    let mut reader = BufReader::new(file);
    if byte_offset > 0 {
        reader
            .seek(SeekFrom::Start(byte_offset))
            .map_err(|e| format!("定位 usage.jsonl 读取位置失败: {}", e))?;
    }

    let conn = app_db.conn();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("开启事务失败: {}", e))?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    // 已消费的完整行末尾字节偏移（半行不推进）
    let mut consumed = byte_offset;
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        buffer.clear();
        let read = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|e| format!("读取 usage.jsonl 失败: {}", e))?;
        if read == 0 {
            break;
        }
        // 末尾没有换行：可能正在写入，留到下一轮再读
        if !buffer.ends_with(b"\n") {
            break;
        }
        consumed += read as u64;
        let Some(row) = parse_magpie_line(&buffer) else {
            continue;
        };
        let request_id = format!("{}:{}", MAGPIE_SOURCE, row.request_id);
        match AppDbService::insert_session_log_on_conn(
            &tx,
            MAGPIE_SOURCE,
            &request_id,
            row.session_id.as_deref().unwrap_or(""),
            &row.model,
            MAGPIE_SOURCE,
            row.input_tokens,
            row.output_tokens,
            row.cache_read,
            row.cache_creation,
            row.created_at,
            row.latency,
            row.first_token_latency,
        ) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[Magpie-SYNC] 插入失败 ({}): {}", row.request_id, e);
                skipped += 1;
            }
        }
    }

    AppDbService::update_session_log_sync_on_conn(
        &tx,
        MAGPIE_SOURCE,
        &file_path_str,
        file_modified,
        consumed as i64,
    )?;
    tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;

    Ok((imported, skipped))
}

/// 执行 Magpie 网关账本增量扫描（source = "Magpie"）
pub fn scan_magpie(app_db: &AppDbService) -> Result<DshScanResult, String> {
    let mut result = DshScanResult {
        files_scanned: 0,
        imported: 0,
        skipped: 0,
        errors: 0,
        total_records: 0,
    };
    let Some(path) = magpie_usage_path(app_db) else {
        result.total_records = app_db.get_session_log_count(MAGPIE_SOURCE).unwrap_or(0);
        return Ok(result);
    };
    if !path.is_file() {
        result.total_records = app_db.get_session_log_count(MAGPIE_SOURCE).unwrap_or(0);
        return Ok(result);
    }
    result.files_scanned = 1;
    match scan_magpie_file_incremental(app_db, &path) {
        Ok((imported, skipped)) => {
            result.imported = imported;
            result.skipped = skipped;
        }
        Err(e) => {
            log::warn!("[Magpie-SCAN] 扫描失败 {:?}: {}", path, e);
            result.errors = 1;
        }
    }
    result.total_records = app_db.get_session_log_count(MAGPIE_SOURCE).unwrap_or(0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_app_db() -> (PathBuf, AppDbService) {
        let dir = std::env::temp_dir().join(format!(
            "ccsa_magpie_scanner_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = AppDbService::new_test_file(&dir.join("pricing.db")).unwrap();
        (dir, db)
    }

    fn gateway_line(model: &str, input: i64, output: i64) -> String {
        format!(
            r#"{{"route_id":1792743290927,"t":"2026-10-23T16:36:56.0193725+08:00","agent":"claude","provider":"deepseek","host":"api.deepseek.com","model":"{model}","in":{input},"out":{output},"cache_read":17408,"cache_write":0,"ms":859,"ttft_ms":605,"status":200,"session":"19963151-6d21-48b9-8191-93e58d30c95b"}}"#
        )
    }

    #[test]
    fn parses_gateway_record() {
        let line = gateway_line("deepseek-flash", 215, 108);
        let row = parse_magpie_line(line.as_bytes()).expect("应当解析成功");
        assert_eq!(row.model, "deepseek-flash");
        assert_eq!(row.input_tokens, 215);
        assert_eq!(row.output_tokens, 108);
        assert_eq!(row.cache_read, 17408);
        assert_eq!(row.cache_creation, 0);
        assert_eq!(row.created_at, parse_time_seconds("2026-10-23T16:36:56.0193725+08:00").unwrap());
        assert_eq!(row.latency, 859);
        assert_eq!(row.first_token_latency, 605);
        assert_eq!(
            row.session_id.as_deref(),
            Some("19963151-6d21-48b9-8191-93e58d30c95b")
        );
        // 同一行重复解析得到相同去重键
        assert_eq!(
            row.request_id,
            parse_magpie_line(line.as_bytes()).unwrap().request_id
        );
    }

    #[test]
    fn skips_reported_and_rejected_and_zero_token() {
        let reported = r#"{"t":"2026-10-23T16:36:56Z","model":"m","in":10,"out":2,"source":"reported"}"#;
        assert!(parse_magpie_line(reported.as_bytes()).is_none());

        let rejected = r#"{"t":"2026-10-23T16:36:56Z","model":"m","in":0,"out":0,"rejected":true}"#;
        assert!(parse_magpie_line(rejected.as_bytes()).is_none());

        let zero = r#"{"t":"2026-10-23T16:36:56Z","model":"m","in":0,"out":0,"cache_read":0,"cache_write":0}"#;
        assert!(parse_magpie_line(zero.as_bytes()).is_none());

        let bad = "not json";
        assert!(parse_magpie_line(bad.as_bytes()).is_none());
    }

    #[test]
    fn falls_back_to_served_or_requested_model_and_empty_session() {
        let line = r#"{"t":"2026-10-23T16:36:56Z","served":"served-model","in":10,"out":2,"ttft_ms":0}"#;
        let row = parse_magpie_line(line.as_bytes()).unwrap();
        assert_eq!(row.model, "served-model");
        assert_eq!(row.first_token_latency, 0);
        assert!(row
            .session_id
            .as_deref()
            .unwrap()
            .starts_with("magpie:"));

        let requested = r#"{"t":"2026-10-23T16:36:56Z","req":"group/model","in":10,"out":2}"#;
        let row = parse_magpie_line(requested.as_bytes()).unwrap();
        assert_eq!(row.model, "group/model");
    }

    #[test]
    fn keeps_handed_on_token_rows() {
        let line = r#"{"t":"2026-10-23T16:36:56Z","model":"m","in":10,"out":2,"handed_on":true,"status":429}"#;
        let row = parse_magpie_line(line.as_bytes()).expect("交接行的 token 仍应计入");
        assert_eq!(row.input_tokens, 10);
    }

    #[test]
    fn incremental_scan_imports_and_dedups() {
        let (dir, db) = temp_app_db();
        let config = dir.join("magpie");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("usage.jsonl"),
            format!("{}\n{}\n", gateway_line("m1", 10, 2), gateway_line("m2", 20, 3)),
        )
        .unwrap();
        db.set_setting(SETTING_MAGPIE_DATA_DIR, &config.to_string_lossy())
            .unwrap();

        let first = scan_magpie(&db).unwrap();
        assert_eq!(first.files_scanned, 1);
        assert_eq!(first.imported, 2);
        assert_eq!(first.total_records, 2);

        // 无变化：零成本跳过
        let second = scan_magpie(&db).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 0);
        assert_eq!(second.total_records, 2);

        // 追加一行：只导入新增
        let mut content = std::fs::read_to_string(config.join("usage.jsonl")).unwrap();
        content.push_str(&format!("{}\n", gateway_line("m3", 30, 4)));
        std::fs::write(config.join("usage.jsonl"), content).unwrap();
        let third = scan_magpie(&db).unwrap();
        assert_eq!(third.imported, 1);
        assert_eq!(third.total_records, 3);

        // 追加完全相同的行：内容哈希去重，不双计
        let mut content = std::fs::read_to_string(config.join("usage.jsonl")).unwrap();
        content.push_str(&gateway_line("m3", 30, 4));
        content.push('\n');
        std::fs::write(config.join("usage.jsonl"), content).unwrap();
        let fourth = scan_magpie(&db).unwrap();
        assert_eq!(fourth.imported, 0);
        assert_eq!(fourth.total_records, 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn incremental_scan_waits_for_half_line() {
        let (dir, db) = temp_app_db();
        let config = dir.join("magpie");
        std::fs::create_dir_all(&config).unwrap();
        let path = config.join("usage.jsonl");
        let complete = gateway_line("m1", 10, 2);
        std::fs::write(&path, format!("{complete}\n")).unwrap();
        db.set_setting(SETTING_MAGPIE_DATA_DIR, &config.to_string_lossy())
            .unwrap();
        assert_eq!(scan_magpie(&db).unwrap().imported, 1);

        // 追加半行：不解析、不推进；补全后下一轮导入
        let half = gateway_line("m2", 20, 3);
        let split = half.len() / 2;
        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push_str(&half[..split]);
        std::fs::write(&path, content).unwrap();
        assert_eq!(scan_magpie(&db).unwrap().imported, 0);

        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push_str(&half[split..]);
        content.push('\n');
        std::fs::write(&path, content).unwrap();
        assert_eq!(scan_magpie(&db).unwrap().imported, 1);
        assert_eq!(db.get_session_log_count(MAGPIE_SOURCE).unwrap(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn truncated_file_resets_cursor_without_double_count() {
        let (dir, db) = temp_app_db();
        let config = dir.join("magpie");
        std::fs::create_dir_all(&config).unwrap();
        let path = config.join("usage.jsonl");
        std::fs::write(
            &path,
            format!("{}\n{}\n", gateway_line("m1", 10, 2), gateway_line("m2", 20, 3)),
        )
        .unwrap();
        db.set_setting(SETTING_MAGPIE_DATA_DIR, &config.to_string_lossy())
            .unwrap();
        assert_eq!(scan_magpie(&db).unwrap().imported, 2);

        // 文件被替换成更短的内容：游标重置，靠哈希主键去重
        std::fs::write(&path, format!("{}\n", gateway_line("m1", 10, 2))).unwrap();
        let next = scan_magpie(&db).unwrap();
        assert_eq!(next.imported, 0);
        assert_eq!(next.total_records, 2);

        // 替换后的新内容照常导入
        std::fs::write(
            &path,
            format!("{}\n{}\n", gateway_line("m1", 10, 2), gateway_line("m9", 90, 9)),
        )
        .unwrap();
        let next = scan_magpie(&db).unwrap();
        assert_eq!(next.imported, 1);
        assert_eq!(db.get_session_log_count(MAGPIE_SOURCE).unwrap(), 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn available_follows_resolved_directory() {
        let (dir, db) = temp_app_db();
        let missing = dir.join("no-magpie");
        db.set_setting(SETTING_MAGPIE_DATA_DIR, &missing.to_string_lossy())
            .unwrap();
        assert!(!magpie_source_available(&db));

        let config = dir.join("magpie");
        std::fs::create_dir_all(&config).unwrap();
        db.set_setting(SETTING_MAGPIE_DATA_DIR, &config.to_string_lossy())
            .unwrap();
        assert!(magpie_source_available(&db));
        let settings = collect_magpie_settings(&db);
        assert_eq!(settings.data_dir.as_deref(), Some(config.to_string_lossy().as_ref()));
        assert!(!settings.usage_file_exists);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 手动冒烟：用真实 magpie usage.jsonl 验证解析与幂等（默认忽略）。
    /// 运行：MAGPIE_REAL_USAGE=<路径> cargo test --lib real_usage_file_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_usage_file_smoke() {
        let real = std::path::PathBuf::from(
            std::env::var("MAGPIE_REAL_USAGE").expect("需要 MAGPIE_REAL_USAGE"),
        );
        let (dir, db) = temp_app_db();
        let config = dir.join("magpie");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::copy(&real, config.join("usage.jsonl")).unwrap();
        db.set_setting(SETTING_MAGPIE_DATA_DIR, &config.to_string_lossy())
            .unwrap();

        let expected_lines = std::fs::read_to_string(&real)
            .unwrap()
            .lines()
            .filter(|line| parse_magpie_line(line.as_bytes()).is_some())
            .count();
        let first = scan_magpie(&db).unwrap();
        println!(
            "real usage: 可导入行={} 首次导入={} 跳过={} 总记录={}",
            expected_lines, first.imported, first.skipped, first.total_records
        );
        assert_eq!(first.imported as usize, expected_lines);
        assert_eq!(first.total_records as usize, expected_lines);

        // 重复扫描幂等
        let second = scan_magpie(&db).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.total_records, first.total_records);

        // 字段抽查：所有记录都有时间与模型，且 provider/source 归到 Magpie
        let conn = db.conn();
        let bad = conn
            .query_row(
                "SELECT COUNT(*) FROM session_request_logs
                 WHERE source = 'Magpie' AND (created_at <= 0 OR model = '' OR provider_id <> 'Magpie' OR latency <= 0)",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(bad, 0);

        std::fs::remove_dir_all(&dir).ok();
    }
}
