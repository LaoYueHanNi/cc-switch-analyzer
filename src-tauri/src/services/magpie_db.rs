//! Magpie 数据源（只读）
//!
//! 以只读连接打开应用自有库 pricing.db，从 session_request_logs 中读取
//! source='Magpie' 的记录（由 magpie_scanner 扫描 magpie 网关账本 usage.jsonl 入库）。
//! 聚合复用 pipeline::aggregate_*，模式为「SQL 取记录 + 内存聚合」的混合型数据源。
//!
//! 只接入网关调用统计：不提供会话管理、项目归属或会话标题，因此所有
//! get_session_* 方法返回空、capabilities.session_management = false。
//! 供应商维度统一归到 Magpie（不暴露 magpie 内部的上游 provider id）。

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

const PROVIDER_ID: &str = "Magpie";
const PROVIDER_NAME: &str = "Magpie";
const SOURCE: &str = "Magpie";

pub struct MagpieDbService {
    db: Option<Mutex<Connection>>,
    db_path: String,
}

impl MagpieDbService {
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
            log::error!("[Magpie] 打开数据库失败 (path={}): {}", file_path, e);
            "打开 Magpie 数据库失败,请检查应用库路径".to_string()
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
            .ok_or_else(|| "Magpie 数据库未打开".to_string())?
            .lock()
            .map_err(|e| format!("Magpie 数据库锁失败: {}", e))
    }

    /// 按 FilterParams 过滤查询 Magpie 记录（SQL 取 source='Magpie' + 内存过滤）。
    fn filtered_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare(
                "SELECT session_id, model, provider_id, created_at,
                        input_tokens, output_tokens, cache_read, cache_creation, latency
                 FROM session_request_logs
                 WHERE source = ?1
                   AND created_at >= ?2 AND created_at < ?3
                 ORDER BY created_at",
            )
            .map_err(|e| format!("查询 Magpie 记录失败: {}", e))?;
        let rows = stmt
            .query_map(
                rusqlite::params![
                    SOURCE,
                    params.from_epoch.filter(|v| *v > 0).unwrap_or(i64::MIN),
                    params.to_epoch.filter(|v| *v > 0).unwrap_or(i64::MAX)
                ],
                |row| {
                    Ok(RawRecord {
                        session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        provider_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        db_type: SOURCE.to_string(),
                        created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                        input_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                        output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                        cache_read: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                        cache_creation: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                        latency: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        is_codex: false,
                    })
                },
            )
            .map_err(|e| format!("查询 Magpie 记录失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            let rec = r.map_err(|e| format!("读取 Magpie 记录失败: {}", e))?;
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

impl DataSource for MagpieDbService {
    fn open(&mut self, path: &str) -> Result<(), String> {
        self.open(path)
    }

    fn close(&mut self) {
        self.close();
    }

    fn is_open(&self) -> bool {
        self.is_open()
    }

    fn get_record_count(&self) -> Result<i64, String> {
        let db = self.db()?;
        db.query_row(
            "SELECT COUNT(*) FROM session_request_logs WHERE source = ?1",
            rusqlite::params![SOURCE],
            |row| row.get(0),
        )
        .map_err(|e| format!("查询 Magpie 记录数失败: {}", e))
    }

    fn get_latest_timestamp(&self) -> Option<i64> {
        self.db().ok().and_then(|db| {
            db.query_row(
                "SELECT MAX(created_at) FROM session_request_logs WHERE source = ?1",
                rusqlite::params![SOURCE],
                |row| row.get::<_, Option<i64>>(0),
            )
            .ok()
        }).flatten()
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
                 WHERE source = ?1 AND model <> '' ORDER BY model",
            )
            .map_err(|e| format!("查询 Magpie 模型失败: {}", e))?;
        let rows = stmt
            .query_map(rusqlite::params![SOURCE], |row| row.get::<_, String>(0))
            .map_err(|e| format!("查询 Magpie 模型失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("读取 Magpie 模型失败: {}", e))?);
        }
        Ok(out)
    }

    fn get_date_range(&self) -> Result<DateRange, String> {
        let db = self.db()?;
        let (min, max) = db
            .query_row(
                "SELECT MIN(created_at), MAX(created_at)
                 FROM session_request_logs WHERE source = ?1",
                rusqlite::params![SOURCE],
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

    fn get_recent_request_logs_raw(
        &self,
        since: Option<i64>,
    ) -> Result<Vec<crate::services::data_source::StreamingRecord>, String> {
        let db = self.db()?;
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match since {
            Some(s) => (
                "SELECT session_id, model, provider_id, created_at,
                        input_tokens, output_tokens, cache_read, cache_creation, latency,
                        first_token_latency
                 FROM session_request_logs
                 WHERE source = ?1 AND created_at >= ?2
                 ORDER BY created_at DESC"
                    .to_string(),
                vec![Box::new(SOURCE), Box::new(s)],
            ),
            None => (
                format!(
                    "SELECT session_id, model, provider_id, created_at,
                            input_tokens, output_tokens, cache_read, cache_creation, latency,
                            first_token_latency
                     FROM session_request_logs
                     WHERE source = ?1
                     ORDER BY created_at DESC
                     LIMIT {}",
                    crate::utils::SESSION_TOP_N
                ),
                vec![Box::new(SOURCE)],
            ),
        };
        let mut stmt = db
            .prepare(&sql)
            .map_err(|e| format!("查询 Magpie 最近请求日志失败: {}", e))?;
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
                    row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    false,
                ))
            })
            .map_err(|e| format!("查询 Magpie 最近请求日志失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("读取 Magpie 最近请求日志失败: {}", e))?);
        }
        Ok(out)
    }

    fn get_filtered_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String> {
        self.filtered_records(params)
    }

    fn capabilities(&self) -> SourceCapabilities {
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

    fn temp_app_db() -> (std::path::PathBuf, String) {
        let path = std::env::temp_dir().join(format!(
            "ccsa_magpie_db_test_{}_{}.db",
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
                created_at INTEGER NOT NULL, latency INTEGER NOT NULL DEFAULT 0,
                first_token_latency INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_request_logs
             (request_id, source, session_id, model, provider_id, input_tokens, output_tokens,
              cache_read, cache_creation, created_at, latency, first_token_latency)
             VALUES (?1, 'Magpie', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                "m-old", "s1", "deepseek-flash", "Magpie", 100, 20, 5000, 10, 1000, 859, 605,
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_request_logs
             (request_id, source, session_id, model, provider_id, input_tokens, output_tokens,
              cache_read, cache_creation, created_at, latency, first_token_latency)
             VALUES (?1, 'Magpie', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                "m-new", "s2", "kimi-k3", "Magpie", 400, 50, 60, 20, 2000, 900, 80,
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_request_logs
             (request_id, source, session_id, model, provider_id, input_tokens, output_tokens,
              cache_read, cache_creation, created_at, latency, first_token_latency)
             VALUES (?1, 'PI', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                "pi-row", "s3", "pi-model", "PI", 1, 1, 1, 1, 3000, 0, 0,
            ],
        )
        .unwrap();
        drop(conn);
        let path_str = path.to_string_lossy().to_string();
        (path, path_str)
    }

    #[test]
    fn queries_only_magpie_rows() {
        let (path, path_str) = temp_app_db();
        let mut svc = MagpieDbService::new();
        svc.open(&path_str).unwrap();

        assert_eq!(svc.get_record_count().unwrap(), 2);
        assert_eq!(svc.get_providers().unwrap()[0].id, "Magpie");
        assert_eq!(svc.get_models().unwrap(), vec!["deepseek-flash", "kimi-k3"]);
        let range = svc.get_date_range().unwrap();
        assert_eq!((range.min, range.max), (1000, 2000));

        let all = svc.get_recent_request_logs_raw(None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].3, 2000);
        assert_eq!(all[0].9, 80);

        // since 为闭式语义：created_at >= since
        let since = svc.get_recent_request_logs_raw(Some(2000)).unwrap();
        assert_eq!(since.len(), 1);

        let params = FilterParams {
            from_epoch: None,
            to_epoch: None,
            tz_offset: None,
            provider_id: None,
            model_id: None,
            ccs_filter_session_apps: None,
        };
        let records = svc.get_filtered_records(&params).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.db_type == "Magpie"));
        assert!(records.iter().all(|r| r.provider_id == "Magpie"));

        // 会话能力关闭：会话相关查询返回空
        assert!(svc.get_session_breakdown(&params).unwrap().is_empty());
        assert!(svc.get_session_request_tokens(&params).unwrap().is_empty());
        assert!(svc.get_session_timestamps(&["s1".into()]).unwrap().is_empty());
        assert!(!svc.capabilities().session_management);
        assert!(svc.capabilities().incremental_scan);

        std::fs::remove_file(&path).ok();
    }
}
