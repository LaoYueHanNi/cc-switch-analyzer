//! MiniMax Code v2(Mavis 桌面端)本地会话用量扫描入库
//!
//! 数据源为 `~/.minimax/v2/sessions/<YYYY>/<MM>/<DD>/<会话目录>/messages.jsonl`,
//! 每个会话目录含 `manifest.json`(sessionId 等)与 `messages.jsonl`(消息流)。
//! 仅 `role=assistant` 且带 `message.usage` 的行携带用量,格式(已实勘验证):
//!
//! ```json
//! {"message_id":"msg-...","turn_id":"...","message":{
//!   "role":"assistant", "content":[...],
//!   "api":"anthropic-messages","provider":"minimax","model":"MiniMax-M3",
//!   "usage":{"input":1028,"output":782,"cacheRead":160991,"cacheWrite":0,"totalTokens":162801,
//!            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},
//!   "stopReason":"stop","timestamp":1787330657320,"responseId":"..."}}
//! ```
//!
//! 解析出的行统一为 [`ParsedRow`],经 [`dsh_scanner::scan_file_incremental`] 增量写入
//! 应用自有库 `pricing.db::session_request_logs`(通用表,source='minimax',
//! request_id = "minimax:" + message_id)。
//!
//! 增量机制与 DSH 完全一致(session_log_sync 表记录 mtime + 行 offset,
//! INSERT OR IGNORE 主键去重),messages.jsonl 被运行中的客户端追加/整文件重写均可容忍。
//!
//! 说明:MiniMax 自家 sqlite(`v2/sqlite/runtime-state.sqlite`)的
//! `local_runtime_token_usage` 表 model 列全为 NULL、cost 全 0,无按模型分析价值,
//! 仅作为扫描结果的行数对账参考,不作为数据源。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::{metadata_modified_nanos, scan_file_incremental, DshScanResult, ParsedRow};

/// MiniMax 数据源标识(也用作 provider_id 与 session_request_logs.source)
pub const MINIMAX_SOURCE: &str = "MiniMax";

/// `~/.minimax` 下会话存储相对路径(v2 布局)
const SESSIONS_REL: &str = "v2/sessions";

/// 解析一行 MiniMax messages.jsonl。
///
/// - 仅 `message.usage` 存在(即 assistant 消息)时返回 Some(ParsedRow)
/// - user / toolResult / 无 usage 行 / 非 JSON 行返回 None(容忍不完整行)
///
/// 行内无 session_id,由调用方从同目录 manifest.json 读取后传入。
fn parse_minimax_line(line: &str, session_id: Option<&str>) -> Option<ParsedRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let message = v.get("message")?;
    let usage = message.get("usage")?;

    let request_id = v.get("message_id").and_then(|i| i.as_str())?.to_string();
    // model: message.model → "unknown"(与 DSH fallback 一致)
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

    // timestamp 毫秒 → 秒
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
        })
    }

/// 从 messages.jsonl 同目录的 manifest.json 读取 sessionId(会话目录稳定标识)。
fn read_session_id_from_manifest(messages_path: &Path) -> Option<String> {
    let manifest_path = messages_path.parent()?.join("manifest.json");
    let text = std::fs::read_to_string(manifest_path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("sessionId").and_then(|s| s.as_str()).map(|s| s.to_string())
}

/// 会话文件判定:messages.jsonl
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

/// 收集 `~/.minimax/v2/sessions/` 下所有 messages.jsonl(排序保证增量稳定)
fn walk_minimax_session_files(minimax_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let sessions_dir = minimax_dir.join(SESSIONS_REL);
    if sessions_dir.is_dir() {
        walk_dir_recursive(&sessions_dir, &mut files);
    }
    files.sort();
    files
}

/// 返回 MiniMax v2 sessions 目录下最新 messages.jsonl 的元数据(供 refresh 的 mtime 检测)
pub fn latest_session_file_mtime(minimax_dir: &Path) -> Option<std::fs::Metadata> {
    walk_minimax_session_files(minimax_dir)
        .into_iter()
        .filter_map(|p| std::fs::metadata(&p).ok())
        .max_by_key(|m| metadata_modified_nanos(m))
}

/// 对指定 MiniMax 数据目录执行扫描(可测入口,不依赖 ~/.minimax 固定路径)。
pub fn scan_minimax_in(app_db: &AppDbService, minimax_dir: &Path) -> Result<DshScanResult, String> {
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
        // 行内无 session_id,从同目录 manifest.json 读取后捕获进闭包
        let session_id = read_session_id_from_manifest(f);
        match scan_file_incremental(app_db, MINIMAX_SOURCE, f, |line| {
            parse_minimax_line(line, session_id.as_deref())
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

/// 扫描默认 MiniMax 目录(~/.minimax)并增量入库。
pub fn scan_minimax(app_db: &AppDbService) -> Result<DshScanResult, String> {
    let dir = crate::utils::get_default_minimax_dir()?;
    scan_minimax_in(app_db, &dir)
}

/// MiniMax 数据是否可用(v2 sessions 目录存在)。
pub fn minimax_source_dir_available() -> bool {
    crate::utils::get_default_minimax_dir()
        .map(|d| d.join(SESSIONS_REL).is_dir())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一行 MiniMax messages.jsonl(assistant 带 usage)
    fn assistant_line(
        id: &str,
        model: &str,
        ts_ms: u64,
        inp: u64,
        out: u64,
        cr: u64,
        cw: u64,
    ) -> String {
        serde_json::json!({
            "message_id": id,
            "turn_id": "turn-1",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}],
                "api": "anthropic-messages",
                "provider": "minimax",
                "model": model,
                "usage": {
                    "input": inp, "output": out,
                    "cacheRead": cr, "cacheWrite": cw,
                    "totalTokens": inp + out + cr + cw,
                    "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}
                },
                "stopReason": "stop",
                "timestamp": ts_ms,
                "responseId": "resp-1"
            }
        })
        .to_string()
    }

    #[test]
    fn test_parse_assistant_message() {
        let line = assistant_line("m1", "MiniMax-M3", 1787330657320, 1028, 782, 160991, 0);
        let m = parse_minimax_line(&line, Some("mvs_s1")).expect("应解析出消息");
        assert_eq!(m.request_id, "m1");
        assert_eq!(m.model, "MiniMax-M3");
        assert_eq!(m.input_tokens, 1028);
        assert_eq!(m.output_tokens, 782);
        assert_eq!(m.cache_read, 160991);
        assert_eq!(m.cache_creation, 0);
        // 毫秒 → 秒
        assert_eq!(m.created_at, 1787330657320 / 1000);
        assert_eq!(m.session_id.as_deref(), Some("mvs_s1"));
    }

    #[test]
    fn test_parse_user_line_returns_none() {
        let line = serde_json::json!({
            "message_id": "u1",
            "turn_id": "turn-1",
            "message": {"role": "user", "content": [{"type": "text", "text": "hi"}]}
        })
        .to_string();
        assert!(parse_minimax_line(&line, None).is_none());
    }

    #[test]
    fn test_parse_tool_result_returns_none() {
        let line = serde_json::json!({
            "message_id": "t1",
            "turn_id": "turn-1",
            "message": {"role": "toolResult", "content": [{"type": "text", "text": "out"}]}
        })
        .to_string();
        assert!(parse_minimax_line(&line, None).is_none());
    }

    #[test]
    fn test_parse_no_session_ok() {
        // session_id 缺失仍可解析(会话视图为空,用量分析正常)
        let line = assistant_line("m2", "MiniMax-M3", 1000, 1, 2, 3, 4);
        let m = parse_minimax_line(&line, None).expect("应解析出消息");
        assert_eq!(m.session_id, None);
        assert_eq!(m.cache_creation, 4);
    }

    #[test]
    fn test_parse_model_missing_fallback() {
        let line = serde_json::json!({
            "message_id": "m3",
            "message": {"role": "assistant", "content": [],
                        "usage": {"input": 10, "output": 0, "cacheRead": 0, "cacheWrite": 0},
                        "timestamp": 2000}
        })
        .to_string();
        let m = parse_minimax_line(&line, None).expect("应解析出消息");
        assert_eq!(m.model, "unknown");
        assert_eq!(m.created_at, 2);
    }

    #[test]
    fn test_parse_invalid_json_returns_none() {
        assert!(parse_minimax_line("not json", None).is_none());
        assert!(parse_minimax_line("", None).is_none());
        assert!(parse_minimax_line("  ", None).is_none());
    }

    #[test]
    fn test_read_session_id_from_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let sess = dir.path().join("2026").join("08").join("22").join("00-session_x");
        std::fs::create_dir_all(&sess).unwrap();
        let msg_path = sess.join("messages.jsonl");
        std::fs::write(&msg_path, "{}").unwrap();

        // 无 manifest → None
        assert!(read_session_id_from_manifest(&msg_path).is_none());

        // 有 manifest → sessionId
        std::fs::write(
            sess.join("manifest.json"),
            serde_json::json!({"schemaVersion": 1, "sessionId": "mvs_abc123", "layout": "v2-final-dated-session"}).to_string(),
        )
        .unwrap();
        assert_eq!(read_session_id_from_manifest(&msg_path).as_deref(), Some("mvs_abc123"));
    }

    #[test]
    fn test_walk_minimax_session_files() {
        let dir = tempfile::tempdir().unwrap();
        let sess = dir.path().join("v2").join("sessions").join("2026").join("08").join("22").join("00-session_x");
        std::fs::create_dir_all(&sess).unwrap();
        std::fs::write(sess.join("messages.jsonl"), b"x").unwrap();
        std::fs::write(sess.join("manifest.json"), b"{}").unwrap();
        std::fs::write(sess.join("notes.txt"), b"x").unwrap();
        let files = walk_minimax_session_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().ends_with("messages.jsonl"));
    }

    /// 把文件 mtime 设到未来,确保 > 已记录的旧 mtime 以触发重扫
    fn set_mtime_future(path: &Path) {
        use std::time::{Duration, SystemTime};
        let future = SystemTime::now() + Duration::from_secs(120);
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.set_modified(future);
        }
    }

    /// 构造一个临时 MiniMax 会话目录(messages.jsonl + manifest.json),返回 messages 路径
    fn make_session_dir(root: &Path, dir_name: &str, session_id: &str, content: &str) -> PathBuf {
        let sess = root.join("v2").join("sessions").join("2026").join("08").join("22").join(dir_name);
        std::fs::create_dir_all(&sess).unwrap();
        std::fs::write(
            sess.join("manifest.json"),
            serde_json::json!({"schemaVersion": 1, "sessionId": session_id, "layout": "v2-final-dated-session"}).to_string(),
        )
        .unwrap();
        let msg_path = sess.join("messages.jsonl");
        std::fs::write(&msg_path, content).unwrap();
        msg_path
    }

    #[test]
    fn test_scan_minimax_incremental() {
        let db = AppDbService::new_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        // 第一个会话:1 条 assistant 用量
        let m1_path = make_session_dir(
            dir.path(),
            "00-session_a",
            "mvs_a",
            &format!("{}\n", assistant_line("a1", "MiniMax-M3", 2_000_000, 100, 10, 50, 0)),
        );

        // 第一次扫描:导入 1 条
        let r1 = scan_minimax_in(&db, dir.path()).unwrap();
        assert_eq!(r1.files_scanned, 1);
        assert_eq!(r1.imported, 1);
        assert_eq!(r1.total_records, 1);

        // 第二次扫描:mtime 未变,跳过
        let r2 = scan_minimax_in(&db, dir.path()).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.total_records, 1);

        // 追加一行 + 触碰 mtime → 再扫导入新行,总数 2,不重复
        std::fs::write(
            &m1_path,
            format!(
                "{}\n{}\n",
                assistant_line("a1", "MiniMax-M3", 2_000_000, 100, 10, 50, 0),
                assistant_line("a2", "MiniMax-M3", 3_000_000, 200, 20, 0, 0)
            ),
        )
        .unwrap();
        set_mtime_future(&m1_path);

        let r3 = scan_minimax_in(&db, dir.path()).unwrap();
        assert_eq!(r3.imported, 1);
        assert_eq!(r3.total_records, 2);

        // 新增第二个会话目录 → 再扫导入,总数 3
        make_session_dir(
            dir.path(),
            "01-session_b",
            "mvs_b",
            &format!("{}\n", assistant_line("b1", "MiniMax-M2.7", 4_000_000, 300, 30, 0, 0)),
        );
        let r4 = scan_minimax_in(&db, dir.path()).unwrap();
        assert_eq!(r4.files_scanned, 2);
        assert_eq!(r4.imported, 1);
        assert_eq!(r4.total_records, 3);

        // 验证 model 与 session_id 落库正确
        let mut stmt = db
            .conn()
            .prepare("SELECT session_id, model FROM session_request_logs WHERE source='MiniMax' ORDER BY created_at")
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![
            ("mvs_a".to_string(), "MiniMax-M3".to_string()),
            ("mvs_a".to_string(), "MiniMax-M3".to_string()),
            ("mvs_b".to_string(), "MiniMax-M2.7".to_string()),
        ]);
    }

    /// 真实数据验证(扫描 ~/.minimax 到内存库,只读不改源文件)
    #[test]
    #[ignore]
    fn test_scan_real_minimax() {
        let dir = match crate::utils::get_default_minimax_dir() {
            Ok(d) if d.is_dir() => d,
            _ => {
                eprintln!("跳过:~/.minimax 不存在");
                return;
            }
        };
        let db = AppDbService::new_in_memory().unwrap();
        let result = scan_minimax_in(&db, &dir).unwrap();
        println!("[REAL-MINIMAX] {:?}", result);
        assert!(result.files_scanned > 0, "应扫描到会话文件");
        assert!(result.total_records > 0, "应导入记录");
        // 验证模型字段有值
        let has_model: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) > 0 FROM session_request_logs WHERE source='MiniMax' AND model <> ''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(has_model, "应至少有一条带 model 的记录");
        // 打印模型分布
        let mut stmt = db
            .conn()
            .prepare("SELECT model, COUNT(*), SUM(input_tokens), SUM(output_tokens) FROM session_request_logs WHERE source='MiniMax' GROUP BY model ORDER BY 2 DESC")
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
        println!("[REAL-MINIMAX] 模型分布:");
        for r in rows {
            println!("{}", r.unwrap());
        }
    }
}