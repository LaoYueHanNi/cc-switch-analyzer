//! PI 数据源（pi 与 oh-my-pi 合并）本地会话用量扫描入库
//!
//! 数据源位置（两者会话 JSONL 格式同构，OMP 为 pi 的超集）：
//! - **pi**: `~/.pi/agent/sessions/`（环境变量 `PI_CODING_AGENT_DIR` 覆盖 agent 目录）
//! - **oh-my-pi (OMP)**: `~/.omp/agent/sessions/`（环境变量 `PI_CONFIG_DIR` 覆盖配置根目录）
//!
//! 目录结构：
//! - 主会话：`sessions/<编码cwd>/<ISO时间戳>_<uuid>.jsonl`
//! - 更深层级为 fork / 子代理转录（如 `<会话文件名>/SleepTest.jsonl`、`run-N/session.jsonl`），
//!   entry 格式相同，递归收集；OMP 的 profiles/*/agent/sessions 与 OMP 自身 stats.db
//!   口径一致，暂不扫描。
//!
//! JSONL：`type=="session"` 头行携带会话 id 与 cwd（OMP 会把 title 行写在最前，
//! 头行不一定在第 1 行，故全文件解析后再回填）。每行一个 entry，计费口径对齐
//! pi 官方 getSessionStats：
//! - `type=="message" && message.role=="assistant"` → `message.usage`（主要用量，
//!   OMP 额外携带 `duration`/`ttft` 毫秒值）
//! - `type=="message" && message.role=="toolResult"` → `message.usage`（工具内嵌
//!   LLM 调用，可选）
//! - `type=="compaction"` / `type=="branch_summary"` → 顶层 `usage`（摘要生成开销）
//!
//! 去重：pi 的 fork 会把源文件全部 entry 原样复制进新文件（同 entry id、同时间戳），
//! request_id 不能含文件标识，用 `"{entry_id}:{毫秒时间戳}"` 跨文件去重，
//! 否则 fork 会话的历史用量会被双计。
//!
//! 解析后的数据增量写入应用自有库 `pricing.db::session_request_logs`，`source = "PI"`。
//! 会话与标题写入 `sessions` 和 `session_titles` 表（OMP 的 title 条目优先，
//! pi 无标题机制时回退首条用户消息文本）。

use std::path::{Path, PathBuf};
use serde_json::Value;

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::{metadata_modified_nanos, DshScanResult, ParsedRow};

/// PI 数据源标识（同时也用作 provider_id 与 session_request_logs.source）
pub const PI_SOURCE: &str = "PI";

/// 会话标题最大长度（超出截断）
const TITLE_MAX_LEN: usize = 60;

/// 从 usage 对象取 token 计数（input, output, cacheRead, cacheWrite）。
/// 个别写入方会把整型写成浮点，as_i64 失败时回退 as_f64 取整。
fn usage_tokens(usage: &Value) -> (i64, i64, i64, i64) {
    let num = |key: &str| -> i64 {
        match usage.get(key) {
            Some(v) => v
                .as_i64()
                .or_else(|| v.as_f64().map(|f| f.round() as i64))
                .unwrap_or(0),
            None => 0,
        }
    };
    (num("input"), num("output"), num("cacheRead"), num("cacheWrite"))
}

/// 毫秒时间戳转秒（兼容已是秒的值）
fn ms_to_secs(ms: i64) -> i64 {
    if ms > 10_000_000_000 {
        ms / 1000
    } else {
        ms
    }
}

/// 解析 ISO 8601 时间串为毫秒时间戳
fn parse_iso_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// 从用户消息 content 提取纯文本（数组取 text 块，字符串直接用）
fn extract_user_text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    let mut parts = Vec::new();
    if let Some(blocks) = content.as_array() {
        for block in blocks {
            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                if !text.trim().is_empty() {
                    parts.push(text.trim().to_string());
                }
            }
        }
    }
    parts.join(" ")
}

/// 清理并截断会话标题
fn clean_title(raw: &str) -> String {
    let one_line: String = raw
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    let trimmed = one_line.trim();
    if trimmed.chars().count() <= TITLE_MAX_LEN {
        trimmed.to_string()
    } else {
        trimmed.chars().take(TITLE_MAX_LEN).collect()
    }
}

/// 解析单行 entry。不依赖会话上下文（session_id 由调用方在头行解析后回填）。
///
/// `file_stem` 用于无 `id` 的历史遗留 entry 的 request_id 兜底
/// （此时无法跨 fork 文件去重，接受极小概率双计）。
pub fn parse_pi_entry(line: &str, line_number: i64, file_stem: &str) -> Option<ParsedRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let entry_type = v.get("type").and_then(|t| t.as_str())?;

    // 定位本行的 usage 对象与 model / 时间戳 / 延迟
    let (usage, model, ts_ms, duration_ms, ttft_ms) = match entry_type {
        "message" => {
            let msg = v.get("message")?;
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let usage = match msg.get("usage") {
                Some(u) if u.is_object() => u,
                _ => return None,
            };
            match role {
                // assistant 为主用量来源；toolResult 为工具内嵌 LLM 调用（可选携带 usage）
                "assistant" | "toolResult" => {}
                _ => return None,
            }
            let model = msg
                .get("model")
                .and_then(|m| m.as_str())
                .filter(|m| !m.trim().is_empty())
                .unwrap_or("unknown")
                .to_string();
            // message.timestamp 为 Unix 毫秒；缺失时回退 entry 顶层 ISO 时间戳
            let ts_ms = msg
                .get("timestamp")
                .and_then(|t| t.as_i64())
                .or_else(|| {
                    v.get("timestamp")
                        .and_then(|t| t.as_str())
                        .and_then(parse_iso_ms)
                });
            let ms_num = |key: &str| -> i64 {
                msg.get(key)
                    .and_then(|d| {
                        d.as_f64().map(|f| f.round() as i64).or_else(|| d.as_i64())
                    })
                    .unwrap_or(0)
            };
            (usage, model, ts_ms, ms_num("duration"), ms_num("ttft"))
        }
        // 摘要生成开销（compaction / branch_summary），顶层 usage，无 model
        "compaction" | "branch_summary" => {
            let usage = match v.get("usage") {
                Some(u) if u.is_object() => u,
                _ => return None,
            };
            let ts_ms = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .and_then(parse_iso_ms);
            (usage, "unknown".to_string(), ts_ms, 0, 0)
        }
        _ => return None,
    };

    let (input_tokens, output_tokens, cache_read, cache_creation) = usage_tokens(usage);
    if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_creation == 0 {
        return None;
    }
    let ts_ms = ts_ms?;

    // entry id 是树节点标识，文件内唯一；fork 复制保留原值，跨文件仍需一致
    let entry_key = v
        .get("id")
        .and_then(|i| i.as_str())
        .filter(|i| !i.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}-L{}", file_stem, line_number));

    Some(ParsedRow {
        request_id: format!("{}:{}", entry_key, ts_ms),
        session_id: None,
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at: ms_to_secs(ts_ms),
        project: String::new(),
        latency: duration_ms,
        first_token_latency: ttft_ms,
    })
}

/// 从 JSONL 文件名提取会话 id 兜底：`<ISO时间戳>_<uuid>.jsonl` 取 uuid 部分
fn session_id_from_file_stem(stem: &str) -> String {
    match stem.split_once('_') {
        Some((_, uuid)) if !uuid.is_empty() => uuid.to_string(),
        _ => stem.to_string(),
    }
}

/// 获取所有候选的 PI 会话根目录（pi 与 OMP 各一）
pub fn get_all_pi_session_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return roots,
    };

    // 1. pi：PI_CODING_AGENT_DIR 直接就是 agent 目录
    let pi_agent_dir = match std::env::var("PI_CODING_AGENT_DIR") {
        Ok(env_dir) if !env_dir.trim().is_empty() => PathBuf::from(env_dir.trim()),
        _ => home.join(".pi").join("agent"),
    };
    let pi_sessions = pi_agent_dir.join("sessions");
    if pi_sessions.is_dir() {
        roots.push(pi_sessions);
    }

    // 2. OMP：PI_CONFIG_DIR 是配置根目录（默认 ~/.omp），会话在其 agent/sessions 下
    let omp_config_dir = match std::env::var("PI_CONFIG_DIR") {
        Ok(env_dir) if !env_dir.trim().is_empty() => PathBuf::from(env_dir.trim()),
        _ => home.join(".omp"),
    };
    let omp_sessions = omp_config_dir.join("agent").join("sessions");
    if omp_sessions.is_dir() && !roots.contains(&omp_sessions) {
        roots.push(omp_sessions);
    }

    roots
}

/// 返回主要/推荐展示的 PI 目录路径（供 get_default_paths 使用）
pub fn primary_pi_dir() -> Option<PathBuf> {
    get_all_pi_session_roots().into_iter().next()
}

/// 检查是否存在 PI 数据源
pub fn pi_source_available() -> bool {
    !get_all_pi_session_roots().is_empty()
}

/// 递归收集根目录下所有会话 JSONL（主会话 2 层，fork/子代理转录更深）
fn collect_jsonl_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

/// 返回所有 PI 会话文件中最新的修改元数据（供 refresh 检测）
pub fn latest_session_file_mtime() -> Option<std::fs::Metadata> {
    let roots = get_all_pi_session_roots();
    let mut files = Vec::new();
    for root in &roots {
        collect_jsonl_files(root, &mut files);
    }
    files
        .into_iter()
        .filter_map(|p| std::fs::metadata(&p).ok())
        .max_by_key(|m| metadata_modified_nanos(m))
}

/// 增量扫描单个会话 JSONL
fn scan_pi_file_incremental(app_db: &AppDbService, path: &Path) -> Result<(u32, u32), String> {
    let file_path_str = path.to_string_lossy().to_string();
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("读取文件元数据失败: {}", e))?;
    let file_modified = metadata_modified_nanos(&metadata);

    let (last_modified, last_offset) = app_db
        .get_session_log_sync_state(PI_SOURCE, &file_path_str)
        .unwrap_or((0, 0));

    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读取会话 JSONL 失败: {}", e))?;

    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session")
        .to_string();

    // 头行不一定在第 1 行（OMP 把 title 行写在最前），先收集行数据与元数据，最后回填
    let mut rows: Vec<(i64, ParsedRow)> = Vec::new();
    let mut session_id = String::new();
    let mut project_dir = String::new();
    let mut title = String::new();

    let mut line_offset = 0i64;
    for line in text.lines() {
        line_offset += 1;
        if line_offset <= last_offset {
            continue;
        }

        if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
            match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "session" => {
                    if session_id.is_empty() {
                        if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
                            session_id = id.trim().to_string();
                        }
                    }
                    if project_dir.is_empty() {
                        if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
                            project_dir = cwd.trim().to_string();
                        }
                    }
                }
                // OMP 的标题条目（原地等宽重写，可能多次更新，取最后一次非空值）
                "title" => {
                    if let Some(t) = v.get("title").and_then(|t| t.as_str()) {
                        if !t.trim().is_empty() {
                            title = clean_title(t);
                        }
                    }
                }
                // pi 无标题机制时回退首条用户消息文本
                "message" => {
                    if title.is_empty() {
                        let role = v
                            .get("message")
                            .and_then(|m| m.get("role"))
                            .and_then(|r| r.as_str())
                            .unwrap_or("");
                        if role == "user" {
                            if let Some(content) =
                                v.get("message").and_then(|m| m.get("content"))
                            {
                                let text = extract_user_text(content);
                                if !text.is_empty() {
                                    title = clean_title(&text);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(row) = parse_pi_entry(line, line_offset, &file_stem) {
            rows.push((line_offset, row));
        }
    }

    // 文件无头行时（异常但容忍）从文件名兜底会话 id
    if session_id.is_empty() {
        session_id = session_id_from_file_stem(&file_stem);
    }
    for (_, row) in rows.iter_mut() {
        row.session_id = Some(session_id.clone());
    }

    let conn = app_db.conn();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("开启事务失败: {}", e))?;

    let mut imported = 0u32;
    let mut skipped = 0u32;

    for (_, row) in &rows {
        let request_id = format!("{}:{}", PI_SOURCE, row.request_id);
        match AppDbService::insert_session_log_on_conn(
            &tx,
            PI_SOURCE,
            &request_id,
            row.session_id.as_deref().unwrap_or(""),
            &row.model,
            PI_SOURCE,
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
                log::warn!("[PI-SYNC] 插入失败 ({}): {}", row.request_id, e);
                skipped += 1;
            }
        }
    }

    // 会话项目归属与标题入库
    if !session_id.is_empty() {
        let _ = tx.execute(
            "INSERT INTO sessions (session_id, project_dir, title, source)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE SET
               project_dir = CASE WHEN sessions.project_dir = '' THEN excluded.project_dir ELSE sessions.project_dir END,
               title = CASE WHEN sessions.title = '' THEN excluded.title ELSE sessions.title END",
            rusqlite::params![
                session_id,
                project_dir,
                title,
                PI_SOURCE,
            ],
        );

        if !title.is_empty() {
            let _ = tx.execute(
                "INSERT OR REPLACE INTO session_titles (session_id, title, source, created_at)
                 VALUES (?1, ?2, ?3, strftime('%s','now'))",
                rusqlite::params![session_id, title, PI_SOURCE],
            );
        }
    }

    AppDbService::update_session_log_sync_on_conn(
        &tx,
        PI_SOURCE,
        &file_path_str,
        file_modified,
        line_offset,
    )?;

    tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;

    Ok((imported, skipped))
}

/// 执行 PI 会话全量/增量扫描（pi 与 OMP 合并，source = "PI"）
pub fn scan_pi(app_db: &AppDbService) -> Result<DshScanResult, String> {
    let roots = get_all_pi_session_roots();
    scan_pi_roots(app_db, &roots)
}

/// 指定根目录列表执行扫描（用于测试与定制）
pub fn scan_pi_roots(app_db: &AppDbService, roots: &[PathBuf]) -> Result<DshScanResult, String> {
    let mut targets = Vec::new();
    for root in roots {
        if root.is_dir() {
            collect_jsonl_files(root, &mut targets);
        }
    }

    // 按路径排序保证扫描稳定性
    targets.sort();

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;

    for target in &targets {
        match scan_pi_file_incremental(app_db, target) {
            Ok((imp, skp)) => {
                imported += imp;
                skipped += skp;
            }
            Err(e) => {
                log::warn!("[PI-SCAN] 扫描文件失败 {:?}: {}", target, e);
                errors += 1;
            }
        }
    }

    let total = app_db.get_session_log_count(PI_SOURCE).unwrap_or(0);

    Ok(DshScanResult {
        files_scanned: targets.len() as u32,
        imported,
        skipped,
        errors,
        total_records: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pi_assistant_entry() {
        // 本机 ~/.pi 真实样例（简化 content）
        let line = r#"{"type":"message","id":"3783b6d9","parentId":"bafaf5ec","timestamp":"2026-09-11T09:19:05.950Z","message":{"role":"assistant","content":[{"type":"text","text":"hi"}],"api":"openai-completions","provider":"kimi-coding","model":"kimi-for-coding","usage":{"input":18334,"output":147,"cacheRead":512,"cacheWrite":0,"totalTokens":18993,"cost":{"input":0.017,"output":0.000588,"cacheRead":9.7e-05,"cacheWrite":0,"total":0.018}},"stopReason":"toolUse","timestamp":1789118338437}}"#;
        let parsed = parse_pi_entry(line, 5, "2026-09-11T09-18-49-049Z_01a08fc3").unwrap();
        assert_eq!(parsed.model, "kimi-for-coding");
        assert_eq!(parsed.input_tokens, 18334);
        assert_eq!(parsed.output_tokens, 147);
        assert_eq!(parsed.cache_read, 512);
        assert_eq!(parsed.cache_creation, 0);
        assert_eq!(parsed.created_at, 1789118338);
        assert_eq!(parsed.request_id, "3783b6d9:1789118338437");
        assert_eq!(parsed.latency, 0);
        assert_eq!(parsed.first_token_latency, 0);
    }

    #[test]
    fn test_parse_omp_assistant_with_ttft() {
        // 本机 ~/.omp 真实样例：duration / ttft 为浮点毫秒
        let line = r#"{"type":"message","id":"e247b6b4","timestamp":"2026-09-19T07:49:53.914Z","message":{"role":"assistant","provider":"deepseek","model":"deepseek-flash","usage":{"input":22963,"output":235,"cacheRead":512,"cacheWrite":0,"totalTokens":23710,"cost":{"total":0.00358}},"stopReason":"toolUse","timestamp":1789804191822,"duration":2065.2477080000026,"ttft":919.6541660000003}}"#;
        let parsed = parse_pi_entry(line, 3, "2026-09-19T07-49-09-564Z_01a0b8a3").unwrap();
        assert_eq!(parsed.model, "deepseek-flash");
        assert_eq!(parsed.latency, 2065);
        assert_eq!(parsed.first_token_latency, 920);
        assert_eq!(parsed.created_at, 1789804191);
    }

    #[test]
    fn test_parse_compaction_entry() {
        // 本机 ~/.pi 真实样例：compaction 顶层 usage、无 model、时间为 ISO 串
        let line = r#"{"type":"compaction","id":"4b7fbf71","parentId":"2f9ac063","timestamp":"2026-09-14T09:15:14.042Z","summary":"...","usage":{"input":73162,"output":6819,"cacheRead":0,"cacheWrite":0,"totalTokens":79981,"cost":{"total":0}}}"#;
        let parsed = parse_pi_entry(line, 10, "some-file").unwrap();
        assert_eq!(parsed.model, "unknown");
        assert_eq!(parsed.input_tokens, 73162);
        assert_eq!(parsed.output_tokens, 6819);
        // 2026-09-14T09:15:14.042Z = 1789804... 实际值由 chrono 解析，校验非零且为秒
        assert!(parsed.created_at > 1_700_000_000 && parsed.created_at < 1_800_000_000);
    }

    #[test]
    fn test_parse_skips_irrelevant_entries() {
        // 用户消息 / 无 usage 的 toolResult / 全 0 usage / 非对象 usage 均跳过
        assert!(parse_pi_entry(r#"{"type":"message","id":"a","message":{"role":"user","content":[{"type":"text","text":"你好"}],"timestamp":1}}"#, 1, "f").is_none());
        assert!(parse_pi_entry(r#"{"type":"message","id":"b","message":{"role":"toolResult","usage":null,"timestamp":1789837572935}}"#, 2, "f").is_none());
        assert!(parse_pi_entry(r#"{"type":"message","id":"c","message":{"role":"assistant","model":"m","usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},"timestamp":1789837572935}}"#, 3, "f").is_none());
        assert!(parse_pi_entry(r#"{"type":"model_change","id":"d","provider":"x","modelId":"y"}"#, 4, "f").is_none());
        // 缺时间戳的 assistant 消息跳过（无法定位到时间轴）
        assert!(parse_pi_entry(r#"{"type":"message","id":"e","message":{"role":"assistant","model":"m","usage":{"input":1,"output":1}}}"#, 5, "f").is_none());
    }

    #[test]
    fn test_fork_dedup_and_incremental() {
        let app_db = AppDbService::new_in_memory().unwrap();

        // 模拟 pi 目录：主会话 + fork 副本（复制全部 entry，头行换新 id）
        let temp_dir = tempfile::tempdir().unwrap();
        let pi_root = temp_dir.path().join("pi_root");
        let session_dir = pi_root.join("--tmp-proj--");
        std::fs::create_dir_all(&session_dir).unwrap();
        let header = r#"{"type":"session","version":3,"id":"01a00000-0000-7000-8000-000000000001","timestamp":"2026-09-01T00:00:00.000Z","cwd":"/tmp/proj"}"#;
        let user_line = r#"{"type":"message","id":"84d4cd80","parentId":null,"timestamp":"2026-09-01T00:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"你好，帮我看个问题"}],"timestamp":1789992001000}}"#;
        let asst_line = r#"{"type":"message","id":"3783b6d9","parentId":"84d4cd80","timestamp":"2026-09-01T00:00:05.950Z","message":{"role":"assistant","provider":"kimi-coding","model":"kimi-for-coding","usage":{"input":18334,"output":147,"cacheRead":512,"cacheWrite":0},"stopReason":"stop","timestamp":1789992005950}}"#;
        let main_file = session_dir.join("2026-09-01T00-00-00-000Z_01a00000-0000-7000-8000-000000000001.jsonl");
        std::fs::write(&main_file, format!("{}\n{}\n{}\n", header, user_line, asst_line)).unwrap();
        // fork：头行是新会话，其后原样复制 entry
        let fork_header = r#"{"type":"session","version":3,"id":"01a00000-0000-7000-8000-000000000002","timestamp":"2026-09-01T01:00:00.000Z","cwd":"/tmp/proj","parentSession":"/pi_root/--tmp-proj--/2026-09-01T00-00-00-000Z_01a00000.jsonl"}"#;
        let fork_file = session_dir.join("2026-09-01T01-00-00-000Z_01a00000-0000-7000-8000-000000000002.jsonl");
        std::fs::write(&fork_file, format!("{}\n{}\n{}\n", fork_header, user_line, asst_line)).unwrap();

        // 3. 首次扫描：fork 复制的 entry 与主文件同 request_id，只计一次
        let r1 = scan_pi_roots(&app_db, &[pi_root.clone()]).unwrap();
        assert_eq!(r1.imported, 1);
        assert_eq!(r1.skipped, 1);
        assert_eq!(r1.total_records, 1);

        // 会话标题取自首条用户消息，项目取自头行 cwd
        let (title, project): (String, String) = {
            let conn = app_db.conn();
            conn.query_row(
                "SELECT title, project_dir FROM sessions WHERE session_id = '01a00000-0000-7000-8000-000000000001'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(title, "你好，帮我看个问题");
        assert_eq!(project, "/tmp/proj");

        // 4. 二次扫描命中增量游标，不再导入
        let r2 = scan_pi_roots(&app_db, &[pi_root]).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.total_records, 1);
    }

    #[test]
    fn test_clean_title() {
        assert_eq!(clean_title("  简单标题  "), "简单标题");
        assert_eq!(clean_title("第一行\n第二行"), "第一行 第二行");
        let long: String = "字".repeat(80);
        assert_eq!(clean_title(&long).chars().count(), TITLE_MAX_LEN);
    }

    #[test]
    fn test_scan_real_machine_pi() {
        if !pi_source_available() {
            println!("[TEST] 本机未检测到 pi / omp 目录，跳过真实数据扫描测试");
            return;
        }
        let roots = get_all_pi_session_roots();
        println!("[TEST] 本机发现 PI 根目录: {:?}", roots);

        // 使用内存数据库执行真实数据扫描
        let app_db = AppDbService::new_in_memory().unwrap();

        let r1 = scan_pi(&app_db).unwrap();
        println!(
            "[TEST] 首次扫描结果: files={}, imported={}, skipped={}, total={}",
            r1.files_scanned, r1.imported, r1.skipped, r1.total_records
        );
        assert!(r1.files_scanned > 0, "应扫描到至少一个会话文件");
        assert!(r1.imported > 0, "应成功导入用量记录");

        // 再次扫描，应命中增量游标全部跳过
        let r2 = scan_pi(&app_db).unwrap();
        assert_eq!(r2.imported, 0, "二次扫描不应导入重复记录");
    }
}
