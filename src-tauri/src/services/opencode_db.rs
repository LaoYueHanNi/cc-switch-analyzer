use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

use crate::models::*;
use crate::utils::*;

// OpenCode SQLite 数据库服务（只读）
pub struct OpenCodeDbService {
    db: Option<Connection>,
    db_path: String,
    latest_timestamp: Option<i64>,
}

unsafe impl Sync for OpenCodeDbService {}

impl OpenCodeDbService {
    pub fn new() -> Self {
        Self {
            db: None,
            db_path: String::new(),
            latest_timestamp: None,
        }
    }

    pub fn open(&mut self, file_path: &str) -> Result<(), String> {
        self.close();
        let conn = Connection::open_with_flags(
            Path::new(file_path),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| {
            log::error!("[OpenCode DB] 打开数据库失败 (path={}): {}", file_path, e);
            "打开数据库失败，请检查文件路径".to_string()
        })?;
        self.db_path = file_path.to_string();
        self.db = Some(conn);
        self.latest_timestamp = self.get_latest_timestamp_internal();
        Ok(())
    }

    pub fn close(&mut self) {
        self.db = None;
        self.db_path = String::new();
    }

    pub fn is_open(&self) -> bool {
        self.db.is_some()
    }

    fn db(&self) -> Result<&Connection, String> {
        self.db
            .as_ref()
            .ok_or_else(|| "数据库未打开".to_string())
    }

    fn collect_rows<T, F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>(
        mut rows: rusqlite::MappedRows<'_, F>,
        context: &str,
    ) -> Result<Vec<T>, String> {
        let mut result = Vec::new();
        while let Some(item) = rows.next() {
            result.push(item.map_err(|e| format!("{}: {}", context, e))?);
        }
        Ok(result)
    }

    // ========== WHERE 子句构建 ==========

    fn tz_date_expr(params: &FilterParams) -> String {
        match params.tz_offset {
            Some(tz) if tz != 0 => {
                let sign = if tz > 0 { "+" } else { "" };
                format!("date(time_created / 1000, 'unixepoch', '{}{} hours')", sign, tz)
            }
            _ => "date(time_created / 1000, 'unixepoch')".to_string(),
        }
    }

    fn build_where_clause(
        params: &FilterParams,
    ) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let mut clauses: Vec<String> = vec![
            "json_extract(data, '$.role') = 'assistant'".to_string(),
            "(CAST(json_extract(data, '$.tokens.input') AS INTEGER) > 0 \
              OR CAST(json_extract(data, '$.tokens.output') AS INTEGER) > 0 \
              OR CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER) > 0 \
              OR CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER) > 0)"
                .to_string(),
        ];
        let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(from_epoch) = params.from_epoch {
            if from_epoch > 0 {
                clauses.push("(time_created / 1000) >= ?".to_string());
                binds.push(Box::new(from_epoch));
            }
        }
        if let Some(to_epoch) = params.to_epoch {
            if to_epoch > 0 {
                clauses.push("(time_created / 1000) < ?".to_string());
                binds.push(Box::new(to_epoch));
            }
        }
        if let Some(ref provider_id) = params.provider_id {
            if !provider_id.is_empty() {
                clauses.push("json_extract(data, '$.providerID') = ?".to_string());
                binds.push(Box::new(provider_id.clone()));
            }
        }
        if let Some(ref model_id) = params.model_id {
            if !model_id.is_empty() {
                clauses.push("json_extract(data, '$.modelID') = ?".to_string());
                binds.push(Box::new(model_id.clone()));
            }
        }

        (format!("WHERE {}", clauses.join(" AND ")), binds)
    }

    // ========== 基础查询 ==========

    pub fn get_record_count(&self) -> Result<i64, String> {
        let db = self.db()?;
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM message WHERE json_extract(data, '$.role') = 'assistant'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询记录数失败: {}", e))?;
        Ok(count)
    }

    fn get_latest_timestamp_internal(&self) -> Option<i64> {
        self.db().ok().and_then(|db| {
            db.query_row(
                "SELECT MAX(time_created / 1000) FROM message WHERE json_extract(data, '$.role') = 'assistant'",
                [],
                |row| row.get(0),
            )
            .ok()
        })
    }

    pub fn get_latest_timestamp(&self) -> Option<i64> {
        self.get_latest_timestamp_internal()
    }

    pub fn get_providers(&self) -> Result<Vec<Provider>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare(
                "SELECT DISTINCT json_extract(data, '$.providerID') AS id
                 FROM message
                 WHERE json_extract(data, '$.role') = 'assistant'
                   AND json_extract(data, '$.providerID') IS NOT NULL
                 ORDER BY id",
            )
            .map_err(|e| format!("查询供应商失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get::<_, Option<String>>(0)?.unwrap_or_default();
                Ok(Provider {
                    name: id.clone(),
                    id,
                })
            })
            .map_err(|e| format!("查询供应商失败: {}", e))?;

        Self::collect_rows(rows, "读取供应商")
    }

    pub fn get_models(&self) -> Result<Vec<String>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare(
                "SELECT DISTINCT json_extract(data, '$.modelID')
                 FROM message
                 WHERE json_extract(data, '$.role') = 'assistant'
                   AND json_extract(data, '$.modelID') IS NOT NULL
                 ORDER BY json_extract(data, '$.modelID')",
            )
            .map_err(|e| format!("查询模型失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, Option<String>>(0))
            .map_err(|e| format!("查询模型失败: {}", e))?;

        Ok(Self::collect_rows(rows, "读取模型")?
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn get_date_range(&self) -> Result<DateRange, String> {
        let db = self.db()?;
        let (min, max): (Option<i64>, Option<i64>) = db
            .query_row(
                "SELECT MIN(time_created / 1000), MAX(time_created / 1000)
                 FROM message
                 WHERE json_extract(data, '$.role') = 'assistant'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("查询日期范围失败: {}", e))?;
        Ok(DateRange { min: min.unwrap_or(0), max: max.unwrap_or(0) })
    }

    // ========== 筛选查询 ==========

    pub fn get_summary(&self, params: &FilterParams) -> Result<SummaryData, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let sql = format!(
            "SELECT
                COUNT(*) AS total_requests,
                SUM(CASE WHEN json_extract(data, '$.finish') IS NOT NULL THEN 1 ELSE 0 END) AS success_count,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS total_input,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS total_output,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS total_cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS total_cache_creation,
                ROUND(AVG(json_extract(data, '$.time.completed') - json_extract(data, '$.time.created')), 0) AS avg_latency
             FROM message {}",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询汇总失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let row = stmt
            .query_row(refs.as_slice(), |row| {
                Ok(SummaryData {
                    total_requests: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    success_count: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    total_input: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    total_output: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    total_cache_read: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    total_cache_creation: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    avg_latency: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                })
            })
            .map_err(|e| format!("查询汇总失败: {}", e))?;
        Ok(row)
    }

    pub fn get_model_breakdown(&self, params: &FilterParams) -> Result<Vec<ModelBreakdown>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let sql = format!(
            "SELECT
                json_extract(data, '$.modelID') AS model,
                COUNT(*) AS requests,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation
             FROM message
             {}
             GROUP BY json_extract(data, '$.modelID')
             ORDER BY requests DESC",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询模型统计失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(ModelBreakdown {
                    model: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    requests: row.get(1)?,
                    input_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询模型统计失败: {}", e))?;

        Self::collect_rows(rows, "读取模型统计")
    }

    pub fn get_provider_breakdown(&self, params: &FilterParams) -> Result<Vec<ProviderBreakdown>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let sql = format!(
            "SELECT
                json_extract(data, '$.providerID') AS provider_name,
                json_extract(data, '$.providerID') AS provider_id,
                COUNT(*) AS requests,
                SUM(CASE WHEN json_extract(data, '$.finish') IS NOT NULL THEN 1 ELSE 0 END) AS successes,
                ROUND(100.0 * SUM(CASE WHEN json_extract(data, '$.finish') IS NOT NULL THEN 1 ELSE 0 END) / COUNT(*), 1) AS success_rate,
                ROUND(AVG(json_extract(data, '$.time.completed') - json_extract(data, '$.time.created')), 0) AS avg_latency
             FROM message
             {}
             GROUP BY json_extract(data, '$.providerID')
             ORDER BY requests DESC",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询供应商统计失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(ProviderBreakdown {
                    provider_name: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    provider_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    requests: row.get(2)?,
                    successes: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    success_rate: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                    avg_latency: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                })
            })
            .map_err(|e| format!("查询供应商统计失败: {}", e))?;

        Self::collect_rows(rows, "读取供应商统计")
    }

    pub fn get_combined_breakdown(&self, params: &FilterParams) -> Result<Vec<CombinedBreakdownRow>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let day_expr = Self::tz_date_expr(params);
        let sql = format!(
            "SELECT
                {} AS day,
                json_extract(data, '$.providerID') AS provider_id,
                json_extract(data, '$.modelID') AS model,
                COUNT(*) AS requests,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation,
                COALESCE(SUM(json_extract(data, '$.time.completed') - json_extract(data, '$.time.created')), 0) AS latency_sum
             FROM message
             {}
             GROUP BY day, json_extract(data, '$.providerID'), json_extract(data, '$.modelID')
             ORDER BY day, json_extract(data, '$.providerID'), json_extract(data, '$.modelID')",
            day_expr, where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询合并统计失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(CombinedBreakdownRow {
                    day: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    provider_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    requests: row.get(3)?,
                    input_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    latency_sum: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                })
            })
            .map_err(|e| format!("查询合并统计失败: {}", e))?;

        Self::collect_rows(rows, "读取合并统计")
    }

    pub fn get_provider_model_tokens(&self, params: &FilterParams) -> Result<Vec<ProviderModelToken>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let sql = format!(
            "SELECT
                json_extract(data, '$.providerID') AS provider_id,
                json_extract(data, '$.modelID') AS model,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation
             FROM message
             {}
             GROUP BY json_extract(data, '$.providerID'), json_extract(data, '$.modelID')",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询供应商模型Token失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(ProviderModelToken {
                    provider_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    input_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询供应商模型Token失败: {}", e))?;

        Self::collect_rows(rows, "读取供应商模型Token")
    }

    pub fn get_daily_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let sql = format!(
            "SELECT
                date(time_created / 1000, 'unixepoch') AS day,
                json_extract(data, '$.modelID') AS model,
                COUNT(*) AS requests,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation,
                ROUND(AVG(json_extract(data, '$.time.completed') - json_extract(data, '$.time.created')), 0) AS avg_latency
             FROM message
             {}
             GROUP BY day, json_extract(data, '$.modelID')
             ORDER BY day",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询每日趋势失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(DailyTrendRow {
                    day: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    requests: row.get(2)?,
                    input_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    avg_latency: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                })
            })
            .map_err(|e| format!("查询每日趋势失败: {}", e))?;

        Self::collect_rows(rows, "读取每日趋势")
    }

    pub fn get_session_breakdown(&self, params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let sql = format!(
            "SELECT
                session_id,
                COUNT(*) AS requests,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation,
                MIN(time_created / 1000) AS first_at,
                MAX(time_created / 1000) AS last_at
             FROM message
             {}
               AND session_id IS NOT NULL AND session_id != ''
             GROUP BY session_id
             ORDER BY requests DESC
             LIMIT {}",
            where_sql, SESSION_TOP_N
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话统计失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionBreakdown {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    requests: row.get(1)?,
                    max_context_width: 0,
                    input_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    first_at: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    last_at: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询会话统计失败: {}", e))?;

        Self::collect_rows(rows, "读取会话统计")
    }

    pub fn get_session_max_context_widths(&self, session_ids: &[String]) -> Result<HashMap<String, i64>, String> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let db = self.db()?;
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT session_id,
                    MAX(CAST(json_extract(data, '$.tokens.input') AS INTEGER)
                        + CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS max_ctx
             FROM message
             WHERE session_id IN ({})
               AND json_extract(data, '$.role') = 'assistant'
               AND (CAST(json_extract(data, '$.tokens.input') AS INTEGER)
                    + CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) > 0
             GROUP BY session_id",
            placeholders.join(",")
        );
        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话最大上下文失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok((row.get::<_, Option<String>>(0)?.unwrap_or_default(), row.get::<_, Option<i64>>(1)?.unwrap_or(0)))
            })
            .map_err(|e| format!("查询会话最大上下文失败: {}", e))?;

        let mut result = HashMap::new();
        for row in rows {
            let (sid, max_ctx) = row.map_err(|e| format!("读取会话最大上下文失败: {}", e))?;
            result.insert(sid, max_ctx);
        }
        Ok(result)
    }

    pub fn get_session_model_tokens(&self, params: &FilterParams) -> Result<Vec<SessionModelToken>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let (sub_where, sub_binds) = Self::build_where_clause(params);
        let sql = format!(
            "SELECT
                session_id,
                json_extract(data, '$.modelID') AS model,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation
             FROM message
             {}
               AND session_id IS NOT NULL AND session_id != ''
               AND session_id IN (
                   SELECT s.session_id FROM message s
                   {}
                     AND s.session_id IS NOT NULL AND s.session_id != ''
                   GROUP BY s.session_id ORDER BY COUNT(*) DESC LIMIT {}
               )
             GROUP BY session_id, json_extract(data, '$.modelID')",
            where_sql, sub_where, SESSION_TOP_N
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话模型Token失败: {}", e))?;
        let mut all_binds: Vec<Box<dyn rusqlite::types::ToSql>> = binds;
        all_binds.extend(sub_binds);
        let refs: Vec<&dyn rusqlite::types::ToSql> = all_binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionModelToken {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    input_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询会话模型Token失败: {}", e))?;

        Self::collect_rows(rows, "读取会话模型Token")
    }

    pub fn get_session_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let (sub_where, sub_binds) = Self::build_where_clause(params);
        let sql = format!(
            "SELECT
                session_id,
                json_extract(data, '$.modelID') AS model,
                (time_created / 1000) AS created_at,
                CAST(json_extract(data, '$.tokens.input') AS INTEGER),
                CAST(json_extract(data, '$.tokens.output') AS INTEGER),
                CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER),
                CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)
             FROM message
             {}
               AND session_id IS NOT NULL AND session_id != ''
               AND session_id IN (
                   SELECT s.session_id FROM message s
                   {}
                     AND s.session_id IS NOT NULL AND s.session_id != ''
                   GROUP BY s.session_id ORDER BY COUNT(*) DESC LIMIT {}
               )
             ORDER BY session_id, time_created",
            where_sql, sub_where, SESSION_TOP_N
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话请求Token失败: {}", e))?;
        let mut all_binds: Vec<Box<dyn rusqlite::types::ToSql>> = binds;
        all_binds.extend(sub_binds);
        let refs: Vec<&dyn rusqlite::types::ToSql> = all_binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionRequestToken {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    created_at: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    input_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询会话请求Token失败: {}", e))?;

        Self::collect_rows(rows, "读取会话请求Token")
    }

    pub fn get_session_request_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionRequestToken>, String> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }
        let db = self.db()?;
        let (where_sql, mut binds) = Self::build_where_clause(params);
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        for sid in session_ids {
            binds.push(Box::new(sid.clone()));
        }
        let sql = format!(
            "SELECT
                session_id,
                json_extract(data, '$.modelID') AS model,
                (time_created / 1000) AS created_at,
                CAST(json_extract(data, '$.tokens.input') AS INTEGER),
                CAST(json_extract(data, '$.tokens.output') AS INTEGER),
                CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER),
                CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)
             FROM message
             {}
               AND session_id IN ({})
             ORDER BY session_id, time_created",
            where_sql, placeholders.join(",")
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话请求Token失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionRequestToken {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    created_at: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    input_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询会话请求Token失败: {}", e))?;

        Self::collect_rows(rows, "读取会话请求Token")
    }

    pub fn get_session_model_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionModelToken>, String> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }
        let db = self.db()?;
        let (where_sql, mut binds) = Self::build_where_clause(params);
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        for sid in session_ids {
            binds.push(Box::new(sid.clone()));
        }
        let sql = format!(
            "SELECT
                session_id,
                json_extract(data, '$.modelID') AS model,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation
             FROM message
             {}
               AND session_id IN ({})
             GROUP BY session_id, json_extract(data, '$.modelID')",
            where_sql, placeholders.join(",")
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话模型Token失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionModelToken {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    input_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询会话模型Token失败: {}", e))?;

        Self::collect_rows(rows, "读取会话模型Token")
    }

    pub fn get_model_context_tier_buckets(
        &self,
        params: &FilterParams,
        tier_thresholds: &[i64],
    ) -> Result<Vec<ModelContextTierBucket>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params);
        let day_expr = Self::tz_date_expr(params);

        let mut case_expr = String::from("CASE");
        for &th in tier_thresholds.iter().rev() {
            case_expr.push_str(&format!(
                " WHEN (CAST(json_extract(data, '$.tokens.input') AS INTEGER) \
                       + CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) >= {} THEN {}",
                th, th
            ));
        }
        case_expr.push_str(" ELSE 0 END");

        let sql = format!(
            "SELECT
                json_extract(data, '$.modelID') AS model,
                {} AS day,
                {} AS context_tier,
                SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation,
                MIN(time_created / 1000) AS representative_epoch
             FROM message
             {}
             GROUP BY json_extract(data, '$.modelID'), day, context_tier",
            day_expr, case_expr, where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询上下文档位聚合失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(ModelContextTierBucket {
                    model: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    day: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    context_tier: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    input_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    representative_epoch: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询上下文档位聚合失败: {}", e))?;

        Self::collect_rows(rows, "读取上下文档位聚合")
    }

    pub fn get_session_timestamps(&self, session_ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let db = self.db()?;
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT session_id, (time_created / 1000) AS created_at
             FROM message
             WHERE session_id IN ({})
               AND json_extract(data, '$.role') = 'assistant'
             ORDER BY session_id, time_created",
            placeholders.join(",")
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话时间戳失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok((row.get::<_, Option<String>>(0)?.unwrap_or_default(), row.get::<_, Option<i64>>(1)?.unwrap_or(0)))
            })
            .map_err(|e| format!("查询会话时间戳失败: {}", e))?;

        let mut result: HashMap<String, Vec<i64>> = HashMap::new();
        for row in rows {
            let (session_id, ts) = row.map_err(|e| format!("读取会话时间戳失败: {}", e))?;
            result.entry(session_id).or_default().push(ts);
        }
        Ok(result)
    }

    pub fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String> {
        let db = self.db()?;
        let one_hour_ago = now_epoch_seconds() - REALTIME_WINDOW_SEC;
        let sql = "
            SELECT ((time_created / 1000) / 10) * 10 AS bucket,
                   COUNT(*) AS requests,
                   SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)) AS input_tokens,
                   SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)) AS output_tokens,
                   SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER)) AS cache_read,
                   SUM(CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER)) AS cache_creation
            FROM message
            WHERE (time_created / 1000) >= ?
              AND json_extract(data, '$.role') = 'assistant'
              AND (CAST(json_extract(data, '$.tokens.input') AS INTEGER) > 0
                   OR CAST(json_extract(data, '$.tokens.output') AS INTEGER) > 0
                   OR CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER) > 0
                   OR CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER) > 0)
            GROUP BY bucket
            ORDER BY bucket";

        let mut stmt = db.prepare(sql).map_err(|e| format!("查询实时趋势失败: {}", e))?;
        let rows = stmt
            .query_map(rusqlite::params![one_hour_ago], |row| {
                Ok(RealtimeBucket {
                    bucket: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    requests: row.get(1)?,
                    input_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询实时趋势失败: {}", e))?;

        Self::collect_rows(rows, "读取实时趋势")
    }

    pub fn get_recent_request_logs_raw(&self, since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64)>, String> {
        let db = self.db()?;
        let base_filter = "json_extract(data, '$.role') = 'assistant'
              AND (CAST(json_extract(data, '$.tokens.input') AS INTEGER) > 0
                   OR CAST(json_extract(data, '$.tokens.output') AS INTEGER) > 0
                   OR CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER) > 0
                   OR CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER) > 0)";

        let sql;
        let params: Vec<Box<dyn rusqlite::types::ToSql>> = match since {
            Some(s) => {
                sql = format!(
                    "SELECT session_id,
                           json_extract(data, '$.modelID'),
                           json_extract(data, '$.providerID'),
                           (time_created / 1000),
                           CAST(json_extract(data, '$.tokens.input') AS INTEGER),
                           CAST(json_extract(data, '$.tokens.output') AS INTEGER),
                           CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER),
                           CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER),
                           (json_extract(data, '$.time.completed') - json_extract(data, '$.time.created'))
                    FROM message
                    WHERE (time_created / 1000) > ?
                      AND {}
                    ORDER BY time_created DESC", base_filter);
                vec![Box::new(s)]
            }
            None => {
                sql = format!(
                    "SELECT session_id,
                           json_extract(data, '$.modelID'),
                           json_extract(data, '$.providerID'),
                           (time_created / 1000),
                           CAST(json_extract(data, '$.tokens.input') AS INTEGER),
                           CAST(json_extract(data, '$.tokens.output') AS INTEGER),
                           CAST(COALESCE(json_extract(data, '$.tokens.cache.read'), 0) AS INTEGER),
                           CAST(COALESCE(json_extract(data, '$.tokens.cache.write'), 0) AS INTEGER),
                           (json_extract(data, '$.time.completed') - json_extract(data, '$.time.created'))
                    FROM message
                    WHERE {}
                    ORDER BY time_created DESC
                    LIMIT 500", base_filter);
                vec![]
            }
        };

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询最近请求日志失败: {}", e))?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
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
            ))
        }).map_err(|e| format!("查询最近请求日志失败: {}", e))?;

        Self::collect_rows(rows, "读取请求日志")
    }

    pub fn get_session_titles_from_db(
        &self,
        session_ids: &[String],
    ) -> Result<HashMap<String, (String, String)>, String> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let db = self.db()?;
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT s.id, s.title, s.directory
             FROM session s
             WHERE s.id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话标题失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| format!("查询会话标题失败: {}", e))?;

        let mut result = HashMap::new();
        for row in rows {
            let (id, title, directory) = row.map_err(|e| format!("读取会话标题失败: {}", e))?;
            result.insert(id, (title, directory));
        }
        Ok(result)
    }
}

impl super::data_source::DataSource for OpenCodeDbService {
    fn open(&mut self, path: &str) -> Result<(), String> { self.open(path) }
    fn close(&mut self) { self.close() }
    fn is_open(&self) -> bool { self.is_open() }
    fn get_record_count(&self) -> Result<i64, String> { self.get_record_count() }
    fn get_latest_timestamp(&self) -> Option<i64> { self.get_latest_timestamp() }
    fn get_providers(&self) -> Result<Vec<Provider>, String> { self.get_providers() }
    fn get_models(&self) -> Result<Vec<String>, String> { self.get_models() }
    fn get_date_range(&self) -> Result<DateRange, String> { self.get_date_range() }
    fn get_summary(&self, params: &FilterParams) -> Result<SummaryData, String> { self.get_summary(params) }
    fn get_model_breakdown(&self, params: &FilterParams) -> Result<Vec<ModelBreakdown>, String> { self.get_model_breakdown(params) }
    fn get_provider_breakdown(&self, params: &FilterParams) -> Result<Vec<ProviderBreakdown>, String> { self.get_provider_breakdown(params) }
    fn get_combined_breakdown(&self, params: &FilterParams) -> Result<Vec<CombinedBreakdownRow>, String> { self.get_combined_breakdown(params) }
    fn get_provider_model_tokens(&self, params: &FilterParams) -> Result<Vec<ProviderModelToken>, String> { self.get_provider_model_tokens(params) }
    fn get_daily_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> { self.get_daily_trend(params) }
    fn get_session_breakdown(&self, params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> { self.get_session_breakdown(params) }
    fn get_session_max_context_widths(&self, ids: &[String]) -> Result<HashMap<String, i64>, String> { self.get_session_max_context_widths(ids) }
    fn get_session_model_tokens(&self, params: &FilterParams) -> Result<Vec<SessionModelToken>, String> { self.get_session_model_tokens(params) }
    fn get_session_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> { self.get_session_request_tokens(params) }
    fn get_session_request_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionRequestToken>, String> { self.get_session_request_tokens_for_ids(params, session_ids) }
    fn get_session_model_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionModelToken>, String> { self.get_session_model_tokens_for_ids(params, session_ids) }
    fn get_session_timestamps(&self, ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String> { self.get_session_timestamps(ids) }
    fn get_model_context_tier_buckets(&self, params: &FilterParams, thresholds: &[i64]) -> Result<Vec<ModelContextTierBucket>, String> { self.get_model_context_tier_buckets(params, thresholds) }
    fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String> { self.get_minute_level_token_trend() }
    fn get_recent_request_logs_raw(&self, since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64)>, String> { self.get_recent_request_logs_raw(since) }
    fn get_session_titles_from_provider(
        &self,
        session_ids: &[String],
    ) -> Option<Result<HashMap<String, (String, String)>, String>> {
        Some(self.get_session_titles_from_db(session_ids))
    }
}
