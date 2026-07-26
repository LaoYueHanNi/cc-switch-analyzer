use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use crate::models::*;
use crate::utils::*;

/// 与 CC-Switch `CACHE_INCLUSIVE_APP_TYPES` 对齐：这些 app 的 `input_tokens` 已含 cache_read。
fn is_cache_inclusive_app(app_type: &str) -> bool {
    matches!(app_type, "codex" | "gemini" | "grokbuild")
}

/// 将 cache-inclusive 口径的 input 归一化为 fresh input（不含 cache_read）。
fn normalize_input_tokens(app_type: &str, raw_input: i64, cache_read: i64) -> i64 {
    if is_cache_inclusive_app(app_type) {
        raw_input.saturating_sub(cache_read)
    } else {
        raw_input
    }
}

// 外部 CC-Switch 数据库服务（只读）
pub struct ExternalDbService {
    db: Option<Mutex<Connection>>,
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
        .map_err(|e| {
            log::error!("[DB] 打开数据库失败 (path={}): {}", file_path, e);
            "打开数据库失败，请检查文件路径".to_string()
        })?;
        self.db_path = file_path.to_string();
        self.db = Some(Mutex::new(conn));
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

    fn db(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.db
            .as_ref()
            .ok_or_else(|| "数据库未打开".to_string())?
            .lock()
            .map_err(|e| format!("数据库锁失败: {}", e))
    }

    /// 收集查询行结果为 Vec<T>，context 用于错误消息前缀
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

    // ========== 构建动态 WHERE 子句 ==========

    fn map_trend_row(row: &rusqlite::Row) -> rusqlite::Result<DailyTrendRow> {
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
    }

    /// 生成带时区偏移的 date 表达式，如 date(created_at, 'unixepoch', '+8 hours')
    ///
    /// 注意: tz_offset 未使用参数绑定，因为 SQLite 的日期函数修饰符
    /// （如 `'+8 hours'`）必须是字符串字面量，绑定参数 `?` 无法被识别为修饰符。
    /// 此处安全无虞，因为 tz_offset 的类型为 `i64`，不可能包含 SQL 注入内容。
    fn tz_date_expr(params: &FilterParams) -> String {
        match params.tz_offset {
            Some(tz) if tz != 0 => {
                let sign = if tz > 0 { "+" } else { "" };
                format!("date(l.created_at, 'unixepoch', '{}{} hours')", sign, tz)
            }
            _ => "date(l.created_at, 'unixepoch')".to_string(),
        }
    }

    fn tz_hour_expr(params: &FilterParams) -> String {
        match params.tz_offset {
            Some(tz) if tz != 0 => {
                let sign = if tz > 0 { "+" } else { "" };
                format!("strftime('%H:00', l.created_at, 'unixepoch', '{}{} hours')", sign, tz)
            }
            _ => "strftime('%H:00', l.created_at, 'unixepoch')".to_string(),
        }
    }

    fn build_where_clause(
        params: &FilterParams,
        aliased: bool,
    ) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let prefix = if aliased { "l." } else { "" };
        let mut clauses: Vec<String> = vec![
            "1=1".to_string(),
            format!(
                "({prefix}input_tokens > 0 OR {prefix}output_tokens > 0 OR {prefix}cache_read_tokens > 0 OR {prefix}cache_creation_tokens > 0)",
                prefix = prefix
            ),
        ];
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
                    id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    name: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                })
            })
            .map_err(|e| format!("查询供应商失败: {}", e))?;

        Self::collect_rows(rows, "读取供应商")
    }

    pub fn get_models(&self) -> Result<Vec<String>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare("SELECT DISTINCT model FROM proxy_request_logs ORDER BY model")
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
                "SELECT MIN(created_at), MAX(created_at) FROM proxy_request_logs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("查询日期范围失败: {}", e))?;
        Ok(DateRange { min: min.unwrap_or(0), max: max.unwrap_or(0) })
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
            .query_map(refs.as_slice(), Self::map_trend_row)
            .map_err(|e| format!("查询每日趋势失败: {}", e))?;

        Self::collect_rows(rows, "读取每日趋势")
    }

    pub fn get_hourly_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let hour_expr = Self::tz_hour_expr(params);
        let sql = format!(
            "SELECT
                {} AS day,
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
            hour_expr, where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询小时趋势失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), Self::map_trend_row)
            .map_err(|e| format!("查询小时趋势失败: {}", e))?;

        Self::collect_rows(rows, "读取小时趋势")
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
                    MAX(CASE WHEN app_type IN ('codex', 'gemini', 'grokbuild') THEN input_tokens
                             ELSE input_tokens + cache_read_tokens END) AS max_ctx
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
        let (where_sql, binds) = Self::build_where_clause(params, true);
        let (sub_where, sub_binds) = Self::build_where_clause(params, false);
        let sql = format!(
            "SELECT
                l.session_id,
                l.model,
                l.provider_id,
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
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    provider_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    input_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    cache_creation: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询会话请求Token失败: {}", e))?;

        Self::collect_rows(rows, "读取会话请求Token")
    }

    /// 按已知 session IDs 查询请求级 token（无子查询）
    pub fn get_session_request_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionRequestToken>, String> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }
        let db = self.db()?;
        let (where_sql, mut binds) = Self::build_where_clause(params, true);
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        for sid in session_ids {
            binds.push(Box::new(sid.clone()));
        }
        let sql = format!(
            "SELECT
                l.session_id,
                l.model,
                l.provider_id,
                l.created_at,
                l.input_tokens,
                l.output_tokens,
                l.cache_read_tokens,
                l.cache_creation_tokens,
                l.app_type
             FROM proxy_request_logs l
             {}
               AND l.session_id IN ({})
             ORDER BY l.session_id, l.created_at",
            where_sql, placeholders.join(",")
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话请求Token失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                let app_type: String = row.get::<_, Option<String>>(8)?.unwrap_or_default();
                let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
                let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
                let input_tokens = normalize_input_tokens(&app_type, raw_input, cache_read);
                Ok(SessionRequestToken {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    provider_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    input_tokens,
                    output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_read,
                    cache_creation: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询会话请求Token失败: {}", e))?;

        Self::collect_rows(rows, "读取会话请求Token")
    }

    /// 按已知 session IDs 查询模型级 token（无子查询）
    pub fn get_session_model_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionModelToken>, String> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }
        let db = self.db()?;
        let (where_sql, mut binds) = Self::build_where_clause(params, true);
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        for sid in session_ids {
            binds.push(Box::new(sid.clone()));
        }
        let sql = format!(
            "SELECT
                l.session_id,
                l.model,
                SUM(l.input_tokens) AS input_tokens,
                SUM(l.output_tokens) AS output_tokens,
                SUM(l.cache_read_tokens) AS cache_read,
                SUM(l.cache_creation_tokens) AS cache_creation,
                l.app_type
             FROM proxy_request_logs l
             {}
               AND l.session_id IN ({})
             GROUP BY l.session_id, l.model, l.app_type",
            where_sql, placeholders.join(",")
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话模型Token失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                let app_type: String = row.get::<_, Option<String>>(6)?.unwrap_or_default();
                let raw_input: i64 = row.get::<_, Option<i64>>(2)?.unwrap_or(0);
                let cache_read: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
                let input_tokens = normalize_input_tokens(&app_type, raw_input, cache_read);
                Ok(SessionModelToken {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    input_tokens,
                    output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    cache_read,
                    cache_creation: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                })
            })
            .map_err(|e| format!("查询会话模型Token失败: {}", e))?;

        Self::collect_rows(rows, "读取会话模型Token")
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
            "SELECT session_id, created_at FROM proxy_request_logs
             WHERE session_id IN ({})
             ORDER BY session_id, created_at",
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
            SELECT (created_at / 10) * 10 AS bucket,
                   COUNT(*) AS requests,
                   SUM(input_tokens) AS input_tokens,
                   SUM(output_tokens) AS output_tokens,
                   SUM(cache_read_tokens) AS cache_read,
                   SUM(cache_creation_tokens) AS cache_creation
            FROM proxy_request_logs
            WHERE created_at >= ?
              AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_tokens > 0 OR cache_creation_tokens > 0)
            GROUP BY bucket
            ORDER BY bucket";

        let mut stmt = db.prepare(sql).map_err(|e| format!("查询实时趋势失败: {}", e))?;
        let rows = stmt
            .query_map(params![one_hour_ago], |row| {
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

    pub fn get_recent_request_logs_raw(&self, since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64, bool)>, String> {
        let db = self.db()?;
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match since {
            Some(s) => ("
                SELECT session_id, model, provider_id, created_at,
                       input_tokens, output_tokens,
                       cache_read_tokens, cache_creation_tokens,
                       latency_ms, app_type
                FROM proxy_request_logs
                WHERE created_at > ?
                  AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_tokens > 0 OR cache_creation_tokens > 0)
                ORDER BY created_at DESC", vec![Box::new(s)]),
            None => ("
                SELECT session_id, model, provider_id, created_at,
                       input_tokens, output_tokens,
                       cache_read_tokens, cache_creation_tokens,
                       latency_ms, app_type
                FROM proxy_request_logs
                WHERE (input_tokens > 0 OR output_tokens > 0 OR cache_read_tokens > 0 OR cache_creation_tokens > 0)
                ORDER BY created_at DESC
                LIMIT 500", vec![]),
        };

        let mut stmt = db.prepare(sql).map_err(|e| format!("查询最近请求日志失败: {}", e))?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            let app_type: String = row.get::<_, Option<String>>(9)?.unwrap_or_default();
            let is_codex = app_type == "codex";
            let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
            let input_tokens = normalize_input_tokens(&app_type, raw_input, cache_read);
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                input_tokens,
                row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                cache_read,
                row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                is_codex,
            ))
        }).map_err(|e| format!("查询最近请求日志失败: {}", e))?;

        Self::collect_rows(rows, "读取请求日志")
    }

    pub fn stream_records(
        &self,
        since: Option<i64>,
        on_record: &mut dyn FnMut((String, String, String, i64, i64, i64, i64, i64, i64, bool)),
    ) -> Result<(), String> {
        let db = self.db()?;
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match since {
            Some(s) => ("
                SELECT session_id, model, provider_id, created_at,
                       input_tokens, output_tokens,
                       cache_read_tokens, cache_creation_tokens,
                       latency_ms, app_type
                FROM proxy_request_logs
                WHERE created_at > ?
                  AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_tokens > 0 OR cache_creation_tokens > 0)
                ORDER BY created_at DESC", vec![Box::new(s)]),
            None => ("
                SELECT session_id, model, provider_id, created_at,
                       input_tokens, output_tokens,
                       cache_read_tokens, cache_creation_tokens,
                       latency_ms, app_type
                FROM proxy_request_logs
                WHERE (input_tokens > 0 OR output_tokens > 0 OR cache_read_tokens > 0 OR cache_creation_tokens > 0)
                ORDER BY created_at DESC
                LIMIT 500", vec![]),
        };

        let mut stmt = db.prepare(sql).map_err(|e| format!("stream_records 准备失败: {}", e))?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut rows = stmt.query_map(params_refs.as_slice(), |row| {
            let app_type: String = row.get::<_, Option<String>>(9)?.unwrap_or_default();
            let is_codex = app_type == "codex";
            let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
            let input_tokens = normalize_input_tokens(&app_type, raw_input, cache_read);
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                input_tokens,
                row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                cache_read,
                row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                is_codex,
            ))
        }).map_err(|e| format!("stream_records 查询失败: {}", e))?;

        while let Some(item) = rows.next() {
            let record = item.map_err(|e| format!("stream_records 读取行失败: {}", e))?;
            on_record(record);
        }
        Ok(())
    }

    pub fn get_filtered_raw_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String> {
        let db = self.db()?;
        let (where_sql, binds) = Self::build_where_clause(params, false);
        let sql = format!(
            "SELECT session_id, model, provider_id, created_at,
                    input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens,
                    latency_ms, app_type
             FROM proxy_request_logs {}
             ORDER BY created_at",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询过滤记录失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            let provider_id: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
            let app_type: String = row.get::<_, Option<String>>(9)?.unwrap_or_default();
            let is_codex = app_type == "codex";
            let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
            // Codex / Gemini / GrokBuild：input 已含 cache_read，归一为 fresh input
            let input_tokens = normalize_input_tokens(&app_type, raw_input, cache_read);
            Ok(RawRecord {
                session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                provider_id: provider_id.clone(),
                created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                input_tokens,
                output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                cache_read,
                cache_creation: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                latency: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                is_codex,
            })
        }).map_err(|e| format!("查询过滤记录失败: {}", e))?;

        Self::collect_rows(rows, "读取过滤记录")
    }
}

impl super::data_source::DataSource for ExternalDbService {
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
    fn get_hourly_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> { self.get_hourly_trend(params) }
    fn get_session_breakdown(&self, params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> { self.get_session_breakdown(params) }
    fn get_session_max_context_widths(&self, ids: &[String]) -> Result<HashMap<String, i64>, String> { self.get_session_max_context_widths(ids) }
    fn get_session_model_tokens(&self, params: &FilterParams) -> Result<Vec<SessionModelToken>, String> { self.get_session_model_tokens(params) }
    fn get_session_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> { self.get_session_request_tokens(params) }
    fn get_session_request_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionRequestToken>, String> { self.get_session_request_tokens_for_ids(params, session_ids) }
    fn get_session_model_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionModelToken>, String> { self.get_session_model_tokens_for_ids(params, session_ids) }
    fn get_session_timestamps(&self, ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String> { self.get_session_timestamps(ids) }
    fn get_model_context_tier_buckets(&self, params: &FilterParams, thresholds: &[i64]) -> Result<Vec<ModelContextTierBucket>, String> { self.get_model_context_tier_buckets(params, thresholds) }
    fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String> { self.get_minute_level_token_trend() }
    fn get_recent_request_logs_raw(&self, since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64, bool)>, String> { self.get_recent_request_logs_raw(since) }
    fn stream_records(&self, since: Option<i64>, on_record: &mut dyn FnMut((String, String, String, i64, i64, i64, i64, i64, i64, bool))) -> Result<(), String> { self.stream_records(since, on_record) }
    fn get_filtered_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String> { self.get_filtered_raw_records(params) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_inclusive_apps_normalize_fresh_input() {
        assert_eq!(normalize_input_tokens("grokbuild", 700, 250), 450);
        assert_eq!(normalize_input_tokens("codex", 1000, 600), 400);
        assert_eq!(normalize_input_tokens("gemini", 800, 300), 500);
        assert_eq!(normalize_input_tokens("claude", 200, 5000), 200);
        assert!(is_cache_inclusive_app("grokbuild"));
        assert!(!is_cache_inclusive_app("claude"));
    }
}
