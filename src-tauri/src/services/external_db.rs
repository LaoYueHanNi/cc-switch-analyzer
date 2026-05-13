use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

use crate::models::*;
use crate::utils::*;

// 外部 CC-Switch 数据库服务（只读）
pub struct ExternalDbService {
    db: Option<Connection>,
    db_path: String,
    latest_timestamp: Option<i64>,
}

impl ExternalDbService {
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
        .map_err(|e| format!("打开数据库失败: {}", e))?;
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

    pub fn path(&self) -> &str {
        &self.db_path
    }

    fn db(&self) -> Result<&Connection, String> {
        self.db
            .as_ref()
            .ok_or_else(|| "数据库未打开".to_string())
    }

    // ========== 构建动态 WHERE 子句 ==========

    /// 生成带时区偏移的 date 表达式，如 date(created_at, 'unixepoch', '+8 hours')
    fn tz_date_expr(params: &FilterParams) -> String {
        match params.tz_offset {
            Some(tz) if tz != 0 => {
                let sign = if tz > 0 { "+" } else { "" };
                format!("date(l.created_at, 'unixepoch', '{}{} hours')", sign, tz)
            }
            _ => "date(l.created_at, 'unixepoch')".to_string(),
        }
    }

    fn build_where_clause(
        params: &FilterParams,
        aliased: bool,
    ) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let prefix = if aliased { "l." } else { "" };
        let mut clauses: Vec<String> = vec!["1=1".to_string()];
        let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(from_epoch) = params.from_epoch {
            if from_epoch > 0 {
                clauses.push(format!("{}created_at >= ?", prefix));
                binds.push(Box::new(from_epoch));
            }
        }
        if let Some(to_epoch) = params.to_epoch {
            if to_epoch > 0 {
                clauses.push(format!("{}created_at < ?", prefix));
                binds.push(Box::new(to_epoch));
            }
        }
        if let Some(ref provider_id) = params.provider_id {
            if !provider_id.is_empty() {
                clauses.push(format!("{}provider_id = ?", prefix));
                binds.push(Box::new(provider_id.clone()));
            }
        }
        if let Some(ref model_id) = params.model_id {
            if !model_id.is_empty() {
                clauses.push(format!("{}model = ?", prefix));
                binds.push(Box::new(model_id.clone()));
            }
        }

        (format!("WHERE {}", clauses.join(" AND ")), binds)
    }

    // ========== 基础查询 ==========

    pub fn get_record_count(&self) -> Result<i64, String> {
        let db = self.db()?;
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| row.get(0))
            .map_err(|e| format!("查询记录数失败: {}", e))?;
        Ok(count)
    }

    fn get_latest_timestamp_internal(&self) -> Option<i64> {
        self.db().ok().and_then(|db| {
            db.query_row(
                "SELECT MAX(created_at) FROM proxy_request_logs",
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
                "SELECT DISTINCT l.provider_id AS id, COALESCE(p.name, l.provider_id) AS name
                 FROM proxy_request_logs l
                 LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
                 ORDER BY name",
            )
            .map_err(|e| format!("查询供应商失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Provider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })
            .map_err(|e| format!("查询供应商失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取供应商失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_models(&self) -> Result<Vec<String>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare("SELECT DISTINCT model FROM proxy_request_logs ORDER BY model")
            .map_err(|e| format!("查询模型失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("查询模型失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取模型失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_date_range(&self) -> Result<DateRange, String> {
        let db = self.db()?;
        let (min, max): (i64, i64) = db
            .query_row(
                "SELECT MIN(created_at), MAX(created_at) FROM proxy_request_logs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("查询日期范围失败: {}", e))?;
        Ok(DateRange { min, max })
    }

    pub fn get_base_pricing(&self) -> Result<Vec<ModelPricing>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare(
                "SELECT model_id,
                    CAST(input_cost_per_million AS REAL) AS input_cost_per_million,
                    CAST(output_cost_per_million AS REAL) AS output_cost_per_million,
                    CAST(cache_read_cost_per_million AS REAL) AS cache_read_cost_per_million,
                    CAST(cache_creation_cost_per_million AS REAL) AS cache_creation_cost_per_million
                 FROM model_pricing ORDER BY model_id",
            )
            .map_err(|e| format!("查询基础定价失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ModelPricing {
                    model_id: row.get("model_id")?,
                    input_cost_per_million: row.get("input_cost_per_million")?,
                    output_cost_per_million: row.get("output_cost_per_million")?,
                    cache_read_cost_per_million: row.get("cache_read_cost_per_million")?,
                    cache_creation_cost_per_million: row.get("cache_creation_cost_per_million")?,
                })
            })
            .map_err(|e| format!("查询基础定价失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            let p = row.map_err(|e| format!("读取定价失败: {}", e))?;
            result.push(p);
        }
        Ok(result)
    }

    // ========== 筛选查询 ==========

    pub fn get_summary(&self, params: &FilterParams) -> Result<SummaryData, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, false);
        let sql = format!(
            "SELECT
                COUNT(*) AS total_requests,
                SUM(CASE WHEN status_code=200 THEN 1 ELSE 0 END) AS success_count,
                SUM(input_tokens) AS total_input,
                SUM(output_tokens) AS total_output,
                SUM(cache_read_tokens) AS total_cache_read,
                SUM(cache_creation_tokens) AS total_cache_creation,
                ROUND(AVG(latency_ms), 0) AS avg_latency
             FROM proxy_request_logs {}",
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
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let sql = format!(
            "SELECT
                l.model,
                COUNT(*) AS requests,
                SUM(l.input_tokens) AS input_tokens,
                SUM(l.output_tokens) AS output_tokens,
                SUM(l.cache_read_tokens) AS cache_read,
                SUM(l.cache_creation_tokens) AS cache_creation
             FROM proxy_request_logs l
             {}
             GROUP BY l.model
             ORDER BY requests DESC",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询模型统计失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(ModelBreakdown {
                    model: row.get(0)?,
                    requests: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    cache_read: row.get(4)?,
                    cache_creation: row.get(5)?,
                })
            })
            .map_err(|e| format!("查询模型统计失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取模型统计失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_provider_breakdown(&self, params: &FilterParams) -> Result<Vec<ProviderBreakdown>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let sql = format!(
            "SELECT
                COALESCE(p.name, l.provider_id) AS provider_name,
                l.provider_id AS provider_id,
                COUNT(*) AS requests,
                SUM(CASE WHEN l.status_code=200 THEN 1 ELSE 0 END) AS successes,
                ROUND(100.0 * SUM(CASE WHEN l.status_code=200 THEN 1 ELSE 0 END) / COUNT(*), 1) AS success_rate,
                ROUND(AVG(l.latency_ms), 0) AS avg_latency
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {}
             GROUP BY l.provider_id
             ORDER BY requests DESC",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询供应商统计失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(ProviderBreakdown {
                    provider_name: row.get(0)?,
                    provider_id: row.get(1)?,
                    requests: row.get(2)?,
                    successes: row.get(3)?,
                    success_rate: row.get(4)?,
                    avg_latency: row.get(5)?,
                })
            })
            .map_err(|e| format!("查询供应商统计失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取供应商统计失败: {}", e))?);
        }
        Ok(result)
    }

    /// 合并查询：一次 GROUP BY (day, provider_id, model) 替代 3 次独立查询
    pub fn get_combined_breakdown(&self, params: &FilterParams) -> Result<Vec<CombinedBreakdownRow>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let day_expr = Self::tz_date_expr(params);
        let sql = format!(
            "SELECT
                {} AS day,
                l.provider_id,
                l.model,
                COUNT(*) AS requests,
                SUM(l.input_tokens) AS input_tokens,
                SUM(l.output_tokens) AS output_tokens,
                SUM(l.cache_read_tokens) AS cache_read,
                SUM(l.cache_creation_tokens) AS cache_creation,
                COALESCE(SUM(l.latency_ms), 0) AS latency_sum
             FROM proxy_request_logs l
             {}
             GROUP BY day, l.provider_id, l.model
             ORDER BY day, l.provider_id, l.model",
            day_expr, where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询合并统计失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(CombinedBreakdownRow {
                    day: row.get(0)?,
                    provider_id: row.get(1)?,
                    model: row.get(2)?,
                    requests: row.get(3)?,
                    input_tokens: row.get(4)?,
                    output_tokens: row.get(5)?,
                    cache_read: row.get(6)?,
                    cache_creation: row.get(7)?,
                    latency_sum: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                })
            })
            .map_err(|e| format!("查询合并统计失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取合并统计失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_provider_model_tokens(&self, params: &FilterParams) -> Result<Vec<ProviderModelToken>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let sql = format!(
            "SELECT
                l.provider_id AS provider_id,
                l.model,
                SUM(l.input_tokens) AS input_tokens,
                SUM(l.output_tokens) AS output_tokens,
                SUM(l.cache_read_tokens) AS cache_read,
                SUM(l.cache_creation_tokens) AS cache_creation
             FROM proxy_request_logs l
             {}
             GROUP BY l.provider_id, l.model",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询供应商模型Token失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(ProviderModelToken {
                    provider_id: row.get(0)?,
                    model: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    cache_read: row.get(4)?,
                    cache_creation: row.get(5)?,
                })
            })
            .map_err(|e| format!("查询供应商模型Token失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取供应商模型Token失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_daily_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let sql = format!(
            "SELECT
                date(l.created_at, 'unixepoch') AS day,
                l.model,
                COUNT(*) AS requests,
                SUM(l.input_tokens) AS input_tokens,
                SUM(l.output_tokens) AS output_tokens,
                SUM(l.cache_read_tokens) AS cache_read,
                SUM(l.cache_creation_tokens) AS cache_creation,
                ROUND(AVG(l.latency_ms), 0) AS avg_latency
             FROM proxy_request_logs l
             {}
             GROUP BY day, l.model
             ORDER BY day",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询每日趋势失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(DailyTrendRow {
                    day: row.get(0)?,
                    model: row.get(1)?,
                    requests: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    cache_read: row.get(5)?,
                    cache_creation: row.get(6)?,
                    avg_latency: row.get(7)?,
                })
            })
            .map_err(|e| format!("查询每日趋势失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取每日趋势失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_session_breakdown(&self, params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let sql = format!(
            "SELECT
                l.session_id,
                COUNT(*) AS requests,
                SUM(l.input_tokens) AS input_tokens,
                SUM(l.output_tokens) AS output_tokens,
                SUM(l.cache_read_tokens) AS cache_read,
                SUM(l.cache_creation_tokens) AS cache_creation,
                MIN(l.created_at) AS first_at,
                MAX(l.created_at) AS last_at
             FROM proxy_request_logs l
             {}
               AND l.session_id IS NOT NULL AND l.session_id != ''
             GROUP BY l.session_id
             ORDER BY requests DESC
             LIMIT {}",
            where_sql, SESSION_TOP_N
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话统计失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionBreakdown {
                    session_id: row.get(0)?,
                    requests: row.get(1)?,
                    max_context_width: 0,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    cache_read: row.get(4)?,
                    cache_creation: row.get(5)?,
                    first_at: row.get(6)?,
                    last_at: row.get(7)?,
                })
            })
            .map_err(|e| format!("查询会话统计失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取会话统计失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_session_max_context_widths(&self, session_ids: &[String]) -> Result<HashMap<String, i64>, String> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let db = self.db()?;
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT session_id, MAX(input_tokens + cache_read_tokens) AS max_ctx
             FROM proxy_request_logs
             WHERE session_id IN ({})
               AND input_tokens + cache_read_tokens > 0
             GROUP BY session_id",
            placeholders.join(",")
        );
        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话最大上下文失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
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
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let (sub_where, sub_binds) = Self::build_where_clause(params, false);
        let sql = format!(
            "SELECT
                l.session_id,
                l.model,
                SUM(l.input_tokens) AS input_tokens,
                SUM(l.output_tokens) AS output_tokens,
                SUM(l.cache_read_tokens) AS cache_read,
                SUM(l.cache_creation_tokens) AS cache_creation
             FROM proxy_request_logs l
             {}
               AND l.session_id IS NOT NULL AND l.session_id != ''
               AND l.session_id IN (
                   SELECT s.session_id FROM proxy_request_logs s
                   {}
                     AND s.session_id IS NOT NULL AND s.session_id != ''
                   GROUP BY s.session_id ORDER BY COUNT(*) DESC LIMIT {}
               )
             GROUP BY l.session_id, l.model",
            where_sql, sub_where, SESSION_TOP_N
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话模型Token失败: {}", e))?;
        let mut all_binds: Vec<Box<dyn rusqlite::types::ToSql>> = binds;
        all_binds.extend(sub_binds);
        let refs: Vec<&dyn rusqlite::types::ToSql> = all_binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionModelToken {
                    session_id: row.get(0)?,
                    model: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    cache_read: row.get(4)?,
                    cache_creation: row.get(5)?,
                })
            })
            .map_err(|e| format!("查询会话模型Token失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取会话模型Token失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_session_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let (sub_where, sub_binds) = Self::build_where_clause(params, false);
        let sql = format!(
            "SELECT
                l.session_id,
                l.model,
                l.created_at,
                l.input_tokens,
                l.output_tokens,
                l.cache_read_tokens,
                l.cache_creation_tokens
             FROM proxy_request_logs l
             {}
               AND l.session_id IS NOT NULL AND l.session_id != ''
               AND l.session_id IN (
                   SELECT s.session_id FROM proxy_request_logs s
                   {}
                     AND s.session_id IS NOT NULL AND s.session_id != ''
                   GROUP BY s.session_id ORDER BY COUNT(*) DESC LIMIT {}
               )
             ORDER BY l.session_id, l.created_at",
            where_sql, sub_where, SESSION_TOP_N
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话请求Token失败: {}", e))?;
        let mut all_binds: Vec<Box<dyn rusqlite::types::ToSql>> = binds;
        all_binds.extend(sub_binds);
        let refs: Vec<&dyn rusqlite::types::ToSql> = all_binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionRequestToken {
                    session_id: row.get(0)?,
                    model: row.get(1)?,
                    created_at: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    cache_read: row.get(5)?,
                    cache_creation: row.get(6)?,
                })
            })
            .map_err(|e| format!("查询会话请求Token失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取会话请求Token失败: {}", e))?);
        }
        Ok(result)
    }

    /// 获取所有请求级 Token 数据（用于全局上下文档位计费）
    pub fn get_all_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let sql = format!(
            "SELECT
                COALESCE(session_id, ''),
                l.model,
                l.created_at,
                l.input_tokens,
                l.output_tokens,
                l.cache_read_tokens,
                l.cache_creation_tokens
             FROM proxy_request_logs l
             {}",
            where_sql
        );
        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询全局请求Token失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(SessionRequestToken {
                    session_id: row.get(0)?,
                    model: row.get(1)?,
                    created_at: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    cache_read: row.get(5)?,
                    cache_creation: row.get(6)?,
                })
            })
            .map_err(|e| format!("查询全局请求Token失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取全局请求Token失败: {}", e))?);
        }
        Ok(result)
    }

    /// 按 (model, day, context_tier_bucket) 预聚合，用于上下文档位费用计算
    pub fn get_model_context_tier_buckets(
        &self,
        params: &FilterParams,
        tier_thresholds: &[i64],
    ) -> Result<Vec<ModelContextTierBucket>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let day_expr = Self::tz_date_expr(params);

        // 构建 CASE 表达式：从高到低匹配
        let mut case_expr = String::from("CASE");
        for &th in tier_thresholds.iter().rev() {
            case_expr.push_str(&format!(
                " WHEN (l.input_tokens + l.cache_read_tokens) >= {} THEN {}",
                th, th
            ));
        }
        case_expr.push_str(" ELSE 0 END");

        let sql = format!(
            "SELECT
                l.model,
                {} AS day,
                {} AS context_tier,
                SUM(l.input_tokens) AS input_tokens,
                SUM(l.output_tokens) AS output_tokens,
                SUM(l.cache_read_tokens) AS cache_read,
                SUM(l.cache_creation_tokens) AS cache_creation,
                MIN(l.created_at) AS representative_epoch
             FROM proxy_request_logs l
             {}
             GROUP BY l.model, day, context_tier",
            day_expr, case_expr, where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询上下文档位聚合失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(ModelContextTierBucket {
                    model: row.get(0)?,
                    day: row.get(1)?,
                    context_tier: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    cache_read: row.get(5)?,
                    cache_creation: row.get(6)?,
                    representative_epoch: row.get(7)?,
                })
            })
            .map_err(|e| format!("查询上下文档位聚合失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取上下文档位聚合失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_session_timestamps(&self, session_ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let db = self.db()?;
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT session_id, created_at FROM proxy_request_logs
             WHERE session_id IN ({})
             ORDER BY session_id, created_at",
            placeholders.join(",")
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话时间戳失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
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
            SELECT (created_at / 10) * 10 AS bucket,
                   COUNT(*) AS requests,
                   SUM(input_tokens) AS input_tokens,
                   SUM(output_tokens) AS output_tokens,
                   SUM(cache_read_tokens) AS cache_read,
                   SUM(cache_creation_tokens) AS cache_creation
            FROM proxy_request_logs
            WHERE created_at >= ?
            GROUP BY bucket
            ORDER BY bucket";

        let mut stmt = db.prepare(sql).map_err(|e| format!("查询实时趋势失败: {}", e))?;
        let rows = stmt
            .query_map(params![one_hour_ago], |row| {
                Ok(RealtimeBucket {
                    bucket: row.get(0)?,
                    requests: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    cache_read: row.get(4)?,
                    cache_creation: row.get(5)?,
                })
            })
            .map_err(|e| format!("查询实时趋势失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取实时趋势失败: {}", e))?);
        }
        Ok(result)
    }

    pub fn get_recent_request_logs_raw(&self, since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64)>, String> {
        let db = self.db()?;
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match since {
            Some(s) => ("
                SELECT session_id, model, provider_id, created_at,
                       input_tokens, output_tokens,
                       cache_read_tokens, cache_creation_tokens,
                       latency_ms
                FROM proxy_request_logs
                WHERE created_at > ?
                ORDER BY created_at DESC", vec![Box::new(s)]),
            None => ("
                SELECT session_id, model, provider_id, created_at,
                       input_tokens, output_tokens,
                       cache_read_tokens, cache_creation_tokens,
                       latency_ms
                FROM proxy_request_logs
                ORDER BY created_at DESC
                LIMIT 500", vec![]),
        };

        let mut stmt = db.prepare(sql).map_err(|e| format!("查询最近请求日志失败: {}", e))?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
            ))
        }).map_err(|e| format!("查询最近请求日志失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取请求日志失败: {}", e))?);
        }
        Ok(result)
    }
}
