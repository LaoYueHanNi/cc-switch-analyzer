//! ZCode 用量扫描入库
//!
//! 只读打开 `~/.zcode/cli/db/db.sqlite` 的 `model_usage`，把分析器需要的字段
//! 增量写入 `pricing.db::session_request_logs`(source='ZCode')。
//!
//! ZCode 会在每次写入用量后 `DELETE … WHERE started_at < now-30d`，源库不保留
//! 跨月历史。本扫描器只追加、不随源库删除：源侧被裁掉的行仍留在应用库。
//!
//! 精简字段（对齐 session_request_logs）：
//! request_id / session_id / model / input(fresh) / output / cache_read /
//! cache_creation / created_at(秒) / latency。不拷会话正文、turn、tool、message。
//!
//! 增量：`session_log_sync.last_line_offset` 存已导入的最大 `started_at`（毫秒），
//! `last_modified` 存源 sqlite mtime 纳秒；mtime 未变则跳过。过滤用
//! `started_at >= cursor`，主键 `ZCode:{id}` 去重。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::{metadata_modified_nanos, DshScanResult};
use crate::utils;

/// 数据源标识（session_request_logs.source / provider_id）
pub const ZCODE_SOURCE: &str = "ZCode";

/// 将 ZCode cache-inclusive 的 input 归一化为 fresh input。
fn fresh_input(raw_input: i64, cache_read: i64) -> i64 {
    raw_input.saturating_sub(cache_read)
}

/// 默认 ZCode sqlite 存在且为文件时视为可扫描。
pub fn zcode_sqlite_available() -> bool {
    resolve_zcode_sqlite().map(|p| p.is_file()).unwrap_or(false)
}

pub fn resolve_zcode_sqlite() -> Result<PathBuf, String> {
    utils::get_default_zcode_db_path()
}

pub fn zcode_sqlite_mtime() -> Option<std::fs::Metadata> {
    resolve_zcode_sqlite()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
}

/// 扫描默认路径的 ZCode sqlite。文件不存在时返回空结果（不报错）。
pub fn scan_zcode(app_db: &AppDbService) -> Result<DshScanResult, String> {
    match resolve_zcode_sqlite() {
        Ok(path) if path.is_file() => scan_zcode_at(app_db, &path),
        _ => Ok(DshScanResult {
            files_scanned: 0,
            imported: 0,
            skipped: 0,
            errors: 0,
            total_records: app_db.get_session_log_count(ZCODE_SOURCE).unwrap_or(0),
        }),
    }
}

/// 扫描指定 ZCode sqlite 到应用库。
pub fn scan_zcode_at(app_db: &AppDbService, sqlite_path: &Path) -> Result<DshScanResult, String> {
    let file_path_str = sqlite_path.to_string_lossy().to_string();
    let metadata = std::fs::metadata(sqlite_path)
        .map_err(|e| format!("读取 ZCode 数据库元数据失败: {}", e))?;
    let mtime = metadata_modified_nanos(&metadata);
    let prev = app_db.get_session_log_sync_state(ZCODE_SOURCE, &file_path_str);

    if let Some((prev_mtime, _)) = prev {
        if prev_mtime == mtime {
            return Ok(DshScanResult {
                files_scanned: 0,
                imported: 0,
                skipped: 0,
                errors: 0,
                total_records: app_db.get_session_log_count(ZCODE_SOURCE).unwrap_or(0),
            });
        }
    }

    let cursor_ms = prev.map(|(_, offset)| offset).unwrap_or(0);
    let src = Connection::open_with_flags(sqlite_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("打开 ZCode 数据库失败: {}", e))?;

    let mut stmt = src
        .prepare(
            "SELECT id, session_id, model_id,
                    input_tokens, output_tokens,
                    cache_read_input_tokens, cache_creation_input_tokens,
                    started_at, COALESCE(duration_ms, 0)
             FROM model_usage
             WHERE status = 'completed'
               AND (input_tokens > 0 OR output_tokens > 0
                    OR cache_read_input_tokens > 0 OR cache_creation_input_tokens > 0)
               AND started_at >= ?
             ORDER BY started_at, id",
        )
        .map_err(|e| format!("查询 ZCode model_usage 失败: {}", e))?;

    let rows = stmt
        .query_map(params![cursor_ms], |row| {
            Ok(ZcodeUsageRow {
                id: row.get::<_, String>(0)?,
                session_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                model: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                raw_input: row.get::<_, i64>(3)?,
                output_tokens: row.get::<_, i64>(4)?,
                cache_read: row.get::<_, i64>(5)?,
                cache_creation: row.get::<_, i64>(6)?,
                started_at: row.get::<_, i64>(7)?,
                latency: row.get::<_, i64>(8)?,
            })
        })
        .map_err(|e| format!("读取 ZCode model_usage 失败: {}", e))?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;
    let mut max_started = cursor_ms;

    let tx = app_db
        .conn()
        .unchecked_transaction()
        .map_err(|e| format!("开启事务失败: {}", e))?;

    for row in rows {
        let rec = match row {
            Ok(r) => r,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        if rec.started_at > max_started {
            max_started = rec.started_at;
        }
        let request_id = format!("{}:{}", ZCODE_SOURCE, rec.id);
        let model = if rec.model.trim().is_empty() {
            "unknown".to_string()
        } else {
            rec.model
        };
        match AppDbService::insert_session_log_on_conn(
            &tx,
            ZCODE_SOURCE,
            &request_id,
            &rec.session_id,
            &model,
            ZCODE_SOURCE,
            fresh_input(rec.raw_input, rec.cache_read),
            rec.output_tokens,
            rec.cache_read,
            rec.cache_creation,
            rec.started_at / 1000,
            rec.latency,
            0,
        ) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(_) => errors += 1,
        }
    }

    AppDbService::update_session_log_sync_on_conn(
        &tx,
        ZCODE_SOURCE,
        &file_path_str,
        mtime,
        max_started,
    )?;
    tx.commit()
        .map_err(|e| format!("提交 ZCode 扫描失败: {}", e))?;

    Ok(DshScanResult {
        files_scanned: 1,
        imported,
        skipped,
        errors,
        total_records: app_db.get_session_log_count(ZCODE_SOURCE).unwrap_or(0),
    })
}

struct ZcodeUsageRow {
    id: String,
    session_id: String,
    model: String,
    raw_input: i64,
    output_tokens: i64,
    cache_read: i64,
    cache_creation: i64,
    started_at: i64,
    latency: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn temp_zcode_sqlite() -> (std::path::PathBuf, Connection) {
        let path = std::env::temp_dir().join(format!(
            "ccsa_zcode_scan_src_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE model_usage (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                duration_ms INTEGER,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        (path, conn)
    }

    fn insert_row(
        conn: &Connection,
        id: &str,
        started_at: i64,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_creation: i64,
        status: &str,
    ) {
        conn.execute(
            "INSERT INTO model_usage (id, session_id, model_id, status, started_at, duration_ms,
                                      input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens)
             VALUES (?1, 'sess-1', 'GLM-5', ?2, ?3, 42, ?4, ?5, ?6, ?7)",
            params![id, status, started_at, input, output, cache_read, cache_creation],
        )
        .unwrap();
    }

    fn bump_mtime(path: &Path) {
        use std::time::{Duration, SystemTime};
        let future = SystemTime::now() + Duration::from_secs(120);
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.set_modified(future);
        }
    }

    #[test]
    fn scan_copies_needed_fields_and_normalizes_input() {
        let db = AppDbService::new_in_memory().unwrap();
        let (src_path, src) = temp_zcode_sqlite();
        insert_row(&src, "u1", 1_000_000, 150, 20, 50, 7, "completed");
        insert_row(&src, "skip-zero", 1_000_001, 0, 0, 0, 0, "completed");
        insert_row(&src, "skip-err", 1_000_002, 10, 10, 0, 0, "error");
        drop(src);

        let r = scan_zcode_at(&db, &src_path).unwrap();
        assert_eq!(r.imported, 1);
        assert_eq!(r.total_records, 1);

        let row: (String, String, i64, i64, i64, i64, i64, i64) = db
            .conn()
            .query_row(
                "SELECT request_id, model, input_tokens, output_tokens, cache_read, cache_creation, created_at, latency
                 FROM session_request_logs WHERE source='ZCode'",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "ZCode:u1");
        assert_eq!(row.1, "GLM-5");
        assert_eq!(row.2, 100); // 150 - 50 fresh input
        assert_eq!(row.3, 20);
        assert_eq!(row.4, 50);
        assert_eq!(row.5, 7);
        assert_eq!(row.6, 1000);
        assert_eq!(row.7, 42);

        std::fs::remove_file(&src_path).ok();
    }

    #[test]
    fn second_scan_skips_when_mtime_unchanged_then_imports_new() {
        let db = AppDbService::new_in_memory().unwrap();
        let (src_path, src) = temp_zcode_sqlite();
        insert_row(&src, "u1", 2_000_000, 10, 1, 0, 0, "completed");
        drop(src);

        let r1 = scan_zcode_at(&db, &src_path).unwrap();
        assert_eq!(r1.imported, 1);

        let r2 = scan_zcode_at(&db, &src_path).unwrap();
        assert_eq!(r2.files_scanned, 0);
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.total_records, 1);

        let src = Connection::open(&src_path).unwrap();
        insert_row(&src, "u2", 3_000_000, 8, 2, 0, 0, "completed");
        drop(src);
        bump_mtime(&src_path);

        let r3 = scan_zcode_at(&db, &src_path).unwrap();
        assert_eq!(r3.imported, 1);
        assert_eq!(r3.total_records, 2);

        std::fs::remove_file(&src_path).ok();
    }

    #[test]
    fn prune_on_source_does_not_delete_our_copy() {
        let db = AppDbService::new_in_memory().unwrap();
        let (src_path, src) = temp_zcode_sqlite();
        insert_row(&src, "old", 1_000_000, 10, 1, 0, 0, "completed");
        insert_row(&src, "new", 9_000_000, 20, 2, 0, 0, "completed");
        drop(src);

        assert_eq!(scan_zcode_at(&db, &src_path).unwrap().imported, 2);

        let src = Connection::open(&src_path).unwrap();
        src.execute("DELETE FROM model_usage WHERE id = 'old'", [])
            .unwrap();
        drop(src);
        bump_mtime(&src_path);

        let r = scan_zcode_at(&db, &src_path).unwrap();
        assert_eq!(r.total_records, 2);
        let ids: Vec<String> = db
            .conn()
            .prepare("SELECT request_id FROM session_request_logs WHERE source='ZCode' ORDER BY request_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(ids, vec!["ZCode:new".to_string(), "ZCode:old".to_string()]);

        std::fs::remove_file(&src_path).ok();
    }
}
