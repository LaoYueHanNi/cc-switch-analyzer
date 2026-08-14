//! DSH token-usage 插件数据扫描入库
//!
//! 扫描插件 **dsh-token-usage**(https://github.com/LaoYueHanNi/dsh-token-usage)
//! 写入的按天 JSONL 数据(默认 `~/.dsh/token-usage/usage-YYYY-MM-DD.jsonl`,
//! 目录解析规则与插件一致:显式 `$DSH_HOME` 优先,否则 `~/.dsh`),
//! 增量写入应用自有库 `pricing.db::session_request_logs`(source='dsh')。
//!
//! 行格式(与插件 usage-record.ts 一致,一行一条成功请求):
//! ```json
//! {"requestId":"<assistant message id>","time":<epoch ms>,"sessionId":"…",
//!  "model":"…","usage":{"inputTokens":N,"outputTokens":N,
//!                        "cacheReadTokens"?:N,"cacheWriteTokens"?:N}}
//! ```
//! - `requestId` 即 assistant message id,与会话扫描提取的 message.id 一致;
//!   入库统一 request_id = "dsh:" + requestId,两种来源请求级去重
//! - `usage` 缺失表示 provider 未报告用量(行仍记录但无计费)→ 解析跳过(全 0)
//! - `cacheReadTokens`/`cacheWriteTokens` 缺省补 0;空 model → "unknown"
//! - `time` 毫秒 → /1000 转秒
//!
//! 增量机制复用 [`crate::services::dsh_scanner::scan_file_incremental`]
//! (session_log_sync 按 (file_path, source) 记 mtime + 行 offset;
//! token-usage 文件路径与 sessions 路径天然不冲突)。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::{metadata_modified_nanos, scan_file_incremental, DshScanResult, ParsedRow, DSH_SOURCE};

/// 插件按天文件名匹配:`usage-YYYY-MM-DD.jsonl`(忽略 state.json 等其他文件)。
pub fn is_plugin_file(name: &str) -> bool {
    const PREFIX: &str = "usage-";
    const SUFFIX: &str = ".jsonl";
    if !name.starts_with(PREFIX) || !name.ends_with(SUFFIX) {
        return false;
    }
    let inner = &name[PREFIX.len()..name.len() - SUFFIX.len()];
    let mut parts = inner.split('-');
    let (Some(y), Some(m), Some(d), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let digits = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    digits(y, 4) && digits(m, 2) && digits(d, 2)
}

/// 收集插件数据目录下全部按天 usage 文件(单层遍历,排序保证增量稳定)。
pub fn walk_plugin_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if is_plugin_file(name) {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

/// 返回插件数据目录下最新 usage 文件的元数据(供 refresh 的 mtime 检测)。
pub fn latest_plugin_file_mtime(dir: &Path) -> Option<std::fs::Metadata> {
    walk_plugin_files(dir)
        .into_iter()
        .filter_map(|p| std::fs::metadata(&p).ok())
        .max_by_key(|m| metadata_modified_nanos(m))
}

/// 解析一行插件记录。
///
/// 返回 None 表示该行不是有效计费记录(跳过):
/// - 空行 / 非 JSON / 结构不合法(requestId、usage 缺失,input/output 非数字)
/// - usage 缺失 → provider 未报告用量,无计费数据
///
/// 缺省桶补 0;空 model → "unknown";time 毫秒 → 秒。
fn parse_plugin_line(line: &str) -> Option<ParsedRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let request_id = v.get("requestId").and_then(|i| i.as_str())?;
    if request_id.is_empty() {
        return None;
    }
    let usage = v.get("usage")?;
    let input_tokens = usage.get("inputTokens").and_then(|n| n.as_u64())? as i64;
    let output_tokens = usage.get("outputTokens").and_then(|n| n.as_u64())? as i64;
    let num = |key: &str| -> i64 {
        usage
            .get(key)
            .and_then(|x| x.as_u64())
            .map(|n| n as i64)
            .unwrap_or(0)
    };
    let cache_read = num("cacheReadTokens");
    let cache_creation = num("cacheWriteTokens");

    let model = v
        .get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let session_id = v
        .get("sessionId")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    // time 毫秒 → 秒
    let time_ms = v.get("time").and_then(|t| t.as_u64()).unwrap_or(0);
    let created_at = (time_ms / 1000) as i64;

    Some(ParsedRow {
        request_id: request_id.to_string(),
        session_id,
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at,
    })
}

/// 对指定插件数据目录执行扫描(可测入口,不依赖固定路径)。
pub fn scan_plugin_in(app_db: &AppDbService, dir: &Path) -> Result<DshScanResult, String> {
    if !dir.is_dir() {
        let total = app_db.get_session_log_count(DSH_SOURCE).unwrap_or(0);
        return Ok(DshScanResult {
            files_scanned: 0,
            imported: 0,
            skipped: 0,
            errors: 0,
            total_records: total,
        });
    }
    let files = walk_plugin_files(dir);
    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;
    for f in &files {
        match scan_file_incremental(app_db, DSH_SOURCE, f, parse_plugin_line) {
            Ok((imp, skp)) => {
                imported += imp;
                skipped += skp;
            }
            Err(e) => {
                log::warn!("[DSH-PLUGIN] 文件处理失败 {}: {}", f.display(), e);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_line(
        id: &str,
        time_ms: u64,
        session: &str,
        model: &str,
        inp: Option<u64>,
        out: Option<u64>,
        cr: Option<u64>,
        cw: Option<u64>,
    ) -> String {
        let mut rec = serde_json::Map::new();
        rec.insert("requestId".into(), serde_json::json!(id));
        rec.insert("time".into(), serde_json::json!(time_ms));
        rec.insert("sessionId".into(), serde_json::json!(session));
        rec.insert("model".into(), serde_json::json!(model));
        if let (Some(i), Some(o)) = (inp, out) {
            let mut usage = serde_json::Map::new();
            usage.insert("inputTokens".into(), serde_json::json!(i));
            usage.insert("outputTokens".into(), serde_json::json!(o));
            if let Some(c) = cr {
                usage.insert("cacheReadTokens".into(), serde_json::json!(c));
            }
            if let Some(c) = cw {
                usage.insert("cacheWriteTokens".into(), serde_json::json!(c));
            }
            rec.insert("usage".into(), serde_json::Value::Object(usage));
        }
        serde_json::Value::Object(rec).to_string()
    }

    #[test]
    fn test_is_plugin_file() {
        assert!(is_plugin_file("usage-2026-08-14.jsonl"));
        assert!(is_plugin_file("usage-2026-01-05.jsonl"));
        assert!(!is_plugin_file("state.json"));
        assert!(!is_plugin_file("usage-2026-8-14.jsonl"));
        assert!(!is_plugin_file("usage-2026-08-14.json"));
        assert!(!is_plugin_file("usage-2026-08-14.jsonl.bak"));
        assert!(!is_plugin_file("session.jsonl"));
        assert!(!is_plugin_file(""));
    }

    #[test]
    fn test_parse_plugin_line_full() {
        let line = plugin_line("rid-1", 1786669114335, "session-abc", "deepseek-v4-flash", Some(9779), Some(102), Some(9856), Some(10));
        let m = parse_plugin_line(&line).expect("应解析出记录");
        assert_eq!(m.request_id, "rid-1");
        assert_eq!(m.session_id.as_deref(), Some("session-abc"));
        assert_eq!(m.model, "deepseek-v4-flash");
        assert_eq!(m.input_tokens, 9779);
        assert_eq!(m.output_tokens, 102);
        assert_eq!(m.cache_read, 9856);
        assert_eq!(m.cache_creation, 10);
        assert_eq!(m.created_at, 1786669114335 / 1000);
    }

    #[test]
    fn test_parse_plugin_line_missing_buckets() {
        // 无缓存桶 → 补 0;空 model → unknown
        let line = plugin_line("rid-2", 1000, "s2", "", Some(10), Some(5), None, None);
        let m = parse_plugin_line(&line).expect("应解析出记录");
        assert_eq!(m.cache_read, 0);
        assert_eq!(m.cache_creation, 0);
        assert_eq!(m.model, "unknown");
    }

    #[test]
    fn test_parse_plugin_line_no_usage_skipped() {
        // usage 缺失(provider 未报告用量)→ 无计费数据,跳过
        let line = plugin_line("rid-3", 1000, "s3", "m", None, None, None, None);
        assert!(parse_plugin_line(&line).is_none());
    }

    #[test]
    fn test_parse_plugin_line_invalid() {
        assert!(parse_plugin_line("not json").is_none());
        assert!(parse_plugin_line("").is_none());
        assert!(parse_plugin_line("   ").is_none());
        // 结构不合法:usage 缺 inputTokens
        let line = r#"{"requestId":"r","time":1,"sessionId":"s","model":"m","usage":{"outputTokens":1}}"#;
        assert!(parse_plugin_line(line).is_none());
        // 空 requestId
        let line = r#"{"requestId":"","time":1,"sessionId":"s","model":"m","usage":{"inputTokens":1,"outputTokens":1}}"#;
        assert!(parse_plugin_line(line).is_none());
    }

    #[test]
    fn test_walk_plugin_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("usage-2026-08-14.jsonl"), "x").unwrap();
        std::fs::write(dir.path().join("usage-2026-08-15.jsonl"), "x").unwrap();
        std::fs::write(dir.path().join("state.json"), "{}").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        let files = walk_plugin_files(dir.path());
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_string();
            n.starts_with("usage-") && n.ends_with(".jsonl")
        }));
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
    fn test_scan_plugin_incremental() {
        let db = AppDbService::new_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let day = dir.path().join("usage-2026-08-14.jsonl");

        std::fs::write(&day, format!("{}\n", plugin_line("p1", 2_000_000, "s1", "deepseek-v4-flash", Some(100), Some(10), Some(50), Some(0)))).unwrap();

        // 第一次扫描:导入 1 条
        let r1 = scan_plugin_in(&db, dir.path()).unwrap();
        assert_eq!(r1.files_scanned, 1);
        assert_eq!(r1.imported, 1);
        assert_eq!(r1.total_records, 1);

        // 第二次扫描:mtime 未变,跳过
        let r2 = scan_plugin_in(&db, dir.path()).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.total_records, 1);

        // 追加一行 + 触碰 mtime → 再扫导入新行,总数 2,不重复
        let content = format!(
            "{}\n{}\n",
            plugin_line("p1", 2_000_000, "s1", "deepseek-v4-flash", Some(100), Some(10), Some(50), Some(0)),
            plugin_line("p2", 3_000_000, "s1", "deepseek-v4-flash", Some(200), Some(20), None, None)
        );
        std::fs::write(&day, &content).unwrap();
        set_mtime_future(&day);

        let r3 = scan_plugin_in(&db, dir.path()).unwrap();
        assert_eq!(r3.imported, 1);
        assert_eq!(r3.total_records, 2);

        // 追加无 usage 行(应跳过不导入),mtime 推进
        let content = format!(
            "{}\n{}\n",
            content,
            plugin_line("p3", 4_000_000, "s1", "m", None, None, None, None)
        );
        std::fs::write(&day, content).unwrap();
        set_mtime_future(&day);
        let r4 = scan_plugin_in(&db, dir.path()).unwrap();
        assert_eq!(r4.imported, 0);
        assert_eq!(r4.total_records, 2);
    }

    /// 多天文件同时读取(模拟很久未打开):全部历史文件导入、旧文件跳过、
    /// 新日期文件与旧文件追加行只增量导入
    #[test]
    fn test_scan_plugin_multiple_days() {
        let db = AppDbService::new_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        // 三个历史日期文件 + 干扰文件(state.json / 非 usage 文件)
        for (name, id) in [
            ("usage-2026-08-10.jsonl", "d10"),
            ("usage-2026-08-11.jsonl", "d11"),
            ("usage-2026-08-12.jsonl", "d12"),
        ] {
            std::fs::write(
                dir.path().join(name),
                format!("{}\n", plugin_line(id, 1_000_000, "s1", "m", Some(10), Some(5), None, None)),
            )
            .unwrap();
        }
        std::fs::write(dir.path().join("state.json"), "{}").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();

        // 首次扫描:三个历史文件全部导入(忽略 state.json / notes.txt)
        let r1 = scan_plugin_in(&db, dir.path()).unwrap();
        assert_eq!(r1.files_scanned, 3);
        assert_eq!(r1.imported, 3);
        assert_eq!(r1.total_records, 3);

        // 再次扫描:mtime 未变,全部跳过
        let r2 = scan_plugin_in(&db, dir.path()).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.total_records, 3);

        // 模拟「很久没打开」:期间新增了后续日期文件,旧文件也追加了行
        std::fs::write(
            dir.path().join("usage-2026-08-15.jsonl"),
            format!("{}\n", plugin_line("d15", 2_000_000, "s1", "m", Some(20), Some(5), None, None)),
        )
        .unwrap();
        let old = dir.path().join("usage-2026-08-10.jsonl");
        let old_content = format!(
            "{}\n{}\n",
            plugin_line("d10", 1_000_000, "s1", "m", Some(10), Some(5), None, None),
            plugin_line("d10b", 1_500_000, "s1", "m", Some(11), Some(5), None, None)
        );
        std::fs::write(&old, old_content).unwrap();
        set_mtime_future(&old);

        let r3 = scan_plugin_in(&db, dir.path()).unwrap();
        assert_eq!(r3.files_scanned, 4);
        assert_eq!(r3.imported, 2); // 新日期文件 1 条 + 旧文件追加 1 条
        assert_eq!(r3.total_records, 5);

        // 行内 time 决定入库时间(与文件名日期无关)
        let times: Vec<i64> = db
            .conn()
            .prepare("SELECT created_at FROM session_request_logs WHERE source='dsh' ORDER BY created_at")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(times, vec![1000, 1000, 1000, 1500, 2000]);
    }

    /// 与会话扫描同 request_id 去重:同一 message id 由两种来源先后入库,只保留一条
    #[test]
    fn test_dedupe_with_session_scanner() {
        let db = AppDbService::new_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        // 先以会话扫描导入 message id = "shared-msg"
        let sessions = dir.path().join("sessions").join("--p--").join("session-1");
        std::fs::create_dir_all(&sessions).unwrap();
        let zst = sessions.join("session.jsonl.zstd");
        let evt = serde_json::json!({
            "type": "assistant/message",
            "time": 2_000_000u64,
            "data": {
                "message": { "id": "shared-msg", "source": { "model": "m" } },
                "usage": { "inputTokens": 10, "outputTokens": 5, "cacheReadTokens": 0, "cacheWriteTokens": 0 }
            }
        })
        .to_string();
        std::fs::write(&zst, zstd::encode_all(evt.as_bytes(), 3).unwrap()).unwrap();
        let r1 = crate::services::dsh_scanner::scan_dsh_in(&db, dir.path()).unwrap();
        assert_eq!(r1.imported, 1);
        assert_eq!(r1.total_records, 1);

        // 再以插件数据导入同 requestId → 主键去重,不新增
        let day = dir.path().join("usage-2026-08-14.jsonl");
        std::fs::write(&day, format!("{}\n", plugin_line("shared-msg", 2_000_000, "s1", "m", Some(10), Some(5), Some(0), Some(0)))).unwrap();
        let r2 = scan_plugin_in(&db, dir.path()).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.skipped, 1);
        assert_eq!(r2.total_records, 1);
    }

    /// 真实数据验证(扫描 ~/.dsh/token-usage 到内存库,只读不改源文件)
    #[test]
    #[ignore]
    fn test_scan_real_plugin_data() {
        let dir = match crate::utils::get_default_dsh_plugin_dir() {
            Ok(d) if d.is_dir() => d,
            _ => {
                eprintln!("跳过:插件数据目录不存在");
                return;
            }
        };
        let db = AppDbService::new_in_memory().unwrap();
        let result = scan_plugin_in(&db, &dir).unwrap();
        println!("[REAL-DSH-PLUGIN] {:?}", result);
        assert!(result.files_scanned > 0, "应扫描到 usage 文件");
        assert!(result.total_records > 0, "应导入记录");
        let has_model: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) > 0 FROM session_request_logs WHERE source='dsh' AND model <> ''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(has_model, "应至少有一条带 model 的记录");
        let mut stmt = db
            .conn()
            .prepare("SELECT model, COUNT(*), SUM(input_tokens), SUM(output_tokens) FROM session_request_logs WHERE source='dsh' GROUP BY model ORDER BY 2 DESC")
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
        println!("[REAL-DSH-PLUGIN] 模型分布:");
        for r in rows {
            println!("{}", r.unwrap());
        }
    }
}
