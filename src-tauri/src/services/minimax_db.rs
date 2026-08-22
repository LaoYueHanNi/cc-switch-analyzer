//! MiniMax Code 数据源(只读)
//!
//! 以只读连接打开应用自有库 pricing.db,从 session_request_logs 中读取 source='minimax'
//! 的记录(由 minimax_scanner 扫描入库)。聚合复用 pipeline::aggregate_*(与 DSH 相同),
//! 模式为「SQL 取记录 + 内存聚合」的混合型数据源。

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};

use crate::models::*;
use crate::services::data_source::{DataSource, SourceCapabilities};
use crate::services::pipeline::{
    aggregate_combined_records, aggregate_daily_trend, aggregate_hourly_trend,
    aggregate_model_breakdown, aggregate_model_context_tier_buckets, aggregate_provider_breakdown,
    aggregate_provider_model_tokens, aggregate_summary,
};

const PROVIDER_ID: &str = "MiniMax";
const PROVIDER_NAME: &str = "MiniMax";

pub struct MinimaxDbService {
    db: Option<Mutex<Connection>>,
    db_path: String,
}

impl MinimaxDbService {
    pub fn new() -> Self {
        Self {
            db: None,
            db_path: String::new(),
        }
    }

    pub fn open(&mut self, file_path: &str) -> Result<(), String> {
        self.close();
        let conn = Connection::open_with_flags(
            std::path::Path::new(file_path),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| {
            log::error!("[MINIMAX] 打开数据库失败 (path={}): {}", file_path, e);
            "打开 MiniMax 数据库失败,请检查应用库路径".to_string()
        })?;
        self.db_path = file_path.to_string();
        self.db = Some(Mutex::new(conn));
        Ok(())
    }

    pub fn close(&mut self) {
        self.db = None;
        self.db_path = String::new();
    }

    pub fn is_open(&self) -> bool {
        self.db.is_some()
    }

    fn db(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.db
            .as_ref()
            .ok_or_else(|| "MiniMax 数据库未打开".to_string())?
            .lock()
            .map_err(|e| format!("MiniMax 数据库锁失败: {}", e))
    }

    /// 按 FilterParams 过滤查询 MiniMax 记录(SQL 取全量 source='minimax' + 内存过滤)。
    fn filtered_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare(
                "SELECT session_id, model, provider_id, created_at,
                        input_tokens, output_tokens, cache_read, cache_creation, latency
                 FROM session_request_logs
                 WHERE source = 'MiniMax'
                 ORDER BY created_at",
            )
            .map_err(|e| format!("查询 MiniMax 记录失败: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RawRecord {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    provider_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    db_type: "MiniMax".to_string(),
                    created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    input_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    latency: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                    is_codex: false,
                })
            })
            .map_err(|e| format!("查询 MiniMax 记录失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            let rec = r.map_err(|e| format!("读取 MiniMax 记录失败: {}", e))?;
            if record_matches_params(&rec, params) {
                out.push(rec);
            }
        }
        Ok(out)
    }
}

fn record_matches_params(record: &RawRecord, params: &FilterParams) -> bool {
    if let Some(from) = params.from_epoch {
        if from > 0 && record.created_at < from {
            return false;
        }
    }
    if let Some(to) = params.to_epoch {
        if to > 0 && record.created_at >= to {
            return false;
        }
    }
    if let Some(ref provider_id) = params.provider_id {
        if !provider_id.is_empty() && record.provider_id != *provider_id {
            return false;
        }
    }
    if let Some(ref model_id) = params.model_id {
        if !model_id.is_empty() && record.model != *model_id {
            return false;
        }
    }
    true
}

impl DataSource for MinimaxDbService {
    fn open(&mut self, path: &str) -> Result<(), String> {
        self.open(path)
    }
    fn close(&mut self) {
        self.close()
    }
    fn is_open(&self) -> bool {
        self.is_open()
    }

    fn get_record_count(&self) -> Result<i64, String> {
        let db = self.db()?;
        db.query_row(
            "SELECT COUNT(*) FROM session_request_logs WHERE source = 'MiniMax'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("查询 MiniMax 记录数失败: {}", e))
    }

    fn get_latest_timestamp(&self) -> Option<i64> {
        // 实时查询(非 open 时缓存):scan 入库新数据后,refresh 的 Phase2 能检测到变化
        self.db()
            .ok()
            .and_then(|db| {
                db.query_row(
                    "SELECT MAX(created_at) FROM session_request_logs WHERE source = 'MiniMax'",
                    [],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .ok()
            })
            .flatten()
    }

    fn get_providers(&self) -> Result<Vec<Provider>, String> {
        Ok(vec![Provider {
            id: PROVIDER_ID.to_string(),
            name: PROVIDER_NAME.to_string(),
        }])
    }

    fn get_models(&self) -> Result<Vec<String>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare(
                "SELECT DISTINCT model FROM session_request_logs
                 WHERE source = 'MiniMax' AND model <> '' ORDER BY model",
            )
            .map_err(|e| format!("查询 MiniMax 模型失败: {}", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("查询 MiniMax 模型失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("读取 MiniMax 模型失败: {}", e))?);
        }
        Ok(out)
    }

    fn get_date_range(&self) -> Result<DateRange, String> {
        let db = self.db()?;
        let (min, max) = db
            .query_row(
                "SELECT MIN(created_at), MAX(created_at)
                 FROM session_request_logs WHERE source = 'MiniMax'",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                    ))
                },
            )
            .unwrap_or((None, None));
        Ok(DateRange {
            min: min.unwrap_or(0),
            max: max.unwrap_or(0),
        })
    }

    fn get_summary(&self, params: &FilterParams) -> Result<SummaryData, String> {
        Ok(aggregate_summary(&self.filtered_records(params)?))
    }

    fn get_model_breakdown(&self, params: &FilterParams) -> Result<Vec<ModelBreakdown>, String> {
        Ok(aggregate_model_breakdown(&self.filtered_records(params)?))
    }

    fn get_provider_breakdown(&self, params: &FilterParams) -> Result<Vec<ProviderBreakdown>, String> {
        let names = HashMap::from([(PROVIDER_ID.to_string(), PROVIDER_NAME.to_string())]);
        Ok(aggregate_provider_breakdown(
            &self.filtered_records(params)?,
            &names,
        ))
    }

    fn get_combined_breakdown(&self, params: &FilterParams) -> Result<Vec<CombinedBreakdownRow>, String> {
        Ok(aggregate_combined_records(
            &self.filtered_records(params)?,
            params.tz_offset.unwrap_or(0),
        ))
    }

    fn get_provider_model_tokens(&self, params: &FilterParams) -> Result<Vec<ProviderModelToken>, String> {
        Ok(aggregate_provider_model_tokens(&self.filtered_records(params)?))
    }

    fn get_daily_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> {
        Ok(aggregate_daily_trend(
            &self.filtered_records(params)?,
            params.tz_offset.unwrap_or(0),
        ))
    }

    fn get_hourly_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> {
        Ok(aggregate_hourly_trend(
            &self.filtered_records(params)?,
            params.tz_offset.unwrap_or(0),
        ))
    }

    // 会话 tab、实时 tab 早期返回空(与 DSH 一致)
    fn get_session_breakdown(&self, _params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> {
        Ok(Vec::new())
    }
    fn get_session_max_context_widths(&self, _ids: &[String]) -> Result<HashMap<String, i64>, String> {
        Ok(HashMap::new())
    }
    fn get_session_model_tokens(&self, _params: &FilterParams) -> Result<Vec<SessionModelToken>, String> {
        Ok(Vec::new())
    }
    fn get_session_request_tokens(&self, _params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> {
        Ok(Vec::new())
    }
    fn get_session_request_tokens_for_ids(
        &self,
        _params: &FilterParams,
        _session_ids: &[String],
    ) -> Result<Vec<SessionRequestToken>, String> {
        Ok(Vec::new())
    }
    fn get_session_model_tokens_for_ids(
        &self,
        _params: &FilterParams,
        _session_ids: &[String],
    ) -> Result<Vec<SessionModelToken>, String> {
        Ok(Vec::new())
    }
    fn get_session_timestamps(&self, _ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String> {
        Ok(HashMap::new())
    }

    fn get_model_context_tier_buckets(
        &self,
        params: &FilterParams,
        thresholds: &[i64],
    ) -> Result<Vec<ModelContextTierBucket>, String> {
        Ok(aggregate_model_context_tier_buckets(
            &self.filtered_records(params)?,
            params.tz_offset.unwrap_or(0),
            thresholds,
            None,
        ))
    }

    fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String> {
        Ok(Vec::new())
    }

    /// 实时记录：从 session_request_logs 取最近请求（流式去重管道由此读取）。
    /// since 为秒级游标（增量轮询）；None 时截断到最近 SESSION_TOP_N 条。
    fn get_recent_request_logs_raw(
        &self,
        since: Option<i64>,
    ) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64, bool)>, String> {
        let db = self.db()?;
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match since {
            Some(s) => (
                format!(
                    "SELECT session_id, model, provider_id, created_at,
                            input_tokens, output_tokens, cache_read, cache_creation, latency
                     FROM session_request_logs
                     WHERE source = '{}' AND created_at > ?
                     ORDER BY created_at DESC",
                    PROVIDER_ID
                ),
                vec![Box::new(s)],
            ),
            None => (
                format!(
                    "SELECT session_id, model, provider_id, created_at,
                            input_tokens, output_tokens, cache_read, cache_creation, latency
                     FROM session_request_logs
                     WHERE source = '{}'
                     ORDER BY created_at DESC
                     LIMIT {}",
                    PROVIDER_ID,
                    crate::utils::SESSION_TOP_N
                ),
                vec![],
            ),
        };
        let mut stmt = db
            .prepare(&sql)
            .map_err(|e| format!("查询 MiniMax 最近请求日志失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                    false,
                ))
            })
            .map_err(|e| format!("查询 MiniMax 最近请求日志失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("读取 MiniMax 最近请求日志失败: {}", e))?);
        }
        Ok(out)
    }

    fn get_filtered_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String> {
        self.filtered_records(params)
    }

    fn capabilities(&self) -> SourceCapabilities {
        // manifest.json 无项目字段（实勘确认），仅支持扫描入库
        SourceCapabilities {
            session_management: false,
            project_attribution: false,
            incremental_scan: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一张带 session_request_logs 表的临时应用库（先建库插入再只读打开，与 external_db 测试同模式）
    fn temp_app_db() -> (std::path::PathBuf, String) {
        let path = std::env::temp_dir().join(format!(
            "ccsa_mm_db_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session_request_logs (
                request_id TEXT PRIMARY KEY, source TEXT, session_id TEXT, model TEXT,
                provider_id TEXT, input_tokens INTEGER, output_tokens INTEGER,
                cache_read INTEGER, cache_creation INTEGER,
                created_at INTEGER NOT NULL, latency INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_request_logs
             (request_id, source, session_id, model, provider_id, input_tokens, output_tokens,
              cache_read, cache_creation, created_at, latency)
             VALUES (?1, 'MiniMax', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "r-old", "s1", "MiniMax-Text-01", "MiniMax", 100, 200, 50, 10, 1000, 300,
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_request_logs
             (request_id, source, session_id, model, provider_id, input_tokens, output_tokens,
              cache_read, cache_creation, created_at, latency)
             VALUES (?1, 'MiniMax', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "r-new", "s1", "MiniMax-M2", "MiniMax", 400, 500, 60, 20, 2000, 900,
            ],
        )
        .unwrap();
        // 其他源的记录不应返回
        conn.execute(
            "INSERT INTO session_request_logs
             (request_id, source, session_id, model, provider_id, input_tokens, output_tokens,
              cache_read, cache_creation, created_at, latency)
             VALUES (?1, 'DSH', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "r-dsh", "s9", "deepseek-chat", "DSH", 1, 1, 0, 0, 1500, 10,
            ],
        )
        .unwrap();
        drop(conn);
        let path_str = path.to_string_lossy().to_string();
        (path, path_str)
    }

    #[test]
    fn recent_request_logs_raw_orders_desc_and_filters_since() {
        let (path, path_str) = temp_app_db();
        let mut svc = MinimaxDbService::new();
        svc.open(&path_str).unwrap();

        // 全量：按 created_at 倒序，仅本源记录，末位 is_codex=false
        let all = svc.get_recent_request_logs_raw(None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].3, 2000);
        assert_eq!(all[1].3, 1000);
        assert!(!all[0].9);
        assert_eq!(all[0].2, "MiniMax");

        // 增量：since 之后（秒级 >）仅新记录
        let fresh = svc.get_recent_request_logs_raw(Some(1000)).unwrap();
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].3, 2000);

        // 列映射：input/output/cache_read/cache_creation/latency 与写入一致
        assert_eq!(all[0].4, 400);
        assert_eq!(all[0].5, 500);
        assert_eq!(all[0].6, 60);
        assert_eq!(all[0].7, 20);
        assert_eq!(all[0].8, 900);

        std::fs::remove_file(&path).ok();
    }
}