use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use crate::models::*;
use crate::utils::*;

/// 将 ZCode cache-inclusive 口径的 input（已含 cache_read）归一化为 fresh input。
///
/// ZCode（GLM）的 `input_tokens` 字段包含 cache_read 部分，与 CC-Switch 中
/// codex/gemini/grokbuild 同属 cache-inclusive 口径。减去 cache_read 后得到
/// fresh input，与 Claude 系列口径统一，确保缓存命中率、费用计算正确。
fn fresh_input(raw_input: i64, cache_read: i64) -> i64 {
    raw_input.saturating_sub(cache_read)
}

// ZCode 数据库服务（只读）
//
// 读取 `~/.zcode/cli/db/db.sqlite` 的 `model_usage` 表。
// 关键差异（相对 CC-Switch `proxy_request_logs`）：
// - 时间戳为毫秒（started_at 等），需 /1000 转秒或 ×1000 绑定
// - 无 providers 表，provider 统一为 "ZCode"（provider_id 不对外暴露）
// - 无 app_type 列；input_tokens 为 cache-inclusive 口径（含 cache_read），
//   读取时通过 fresh_input() 归一化为 fresh input，与 Claude 系列口径统一
// - 用 status='completed' 过滤有效请求
pub struct ZCodeDbService {
    db: Option<Mutex<Connection>>,
    db_path: String,
    latest_timestamp: Option<i64>,
}

impl ZCodeDbService {
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
            log::error!("[DB] 打开 ZCode 数据库失败 (path={}): {}", file_path, e);
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

    /// 生成带时区偏移的 date 表达式。
    /// ZCode started_at 为毫秒，需 /1000 后配合 'unixepoch'。
    fn tz_date_expr(params: &FilterParams) -> String {
        match params.tz_offset {
            Some(tz) if tz != 0 => {
                let sign = if tz > 0 { "+" } else { "" };
                format!("date(m.started_at/1000, 'unixepoch', '{}{} hours')", sign, tz)
            }
            _ => "date(m.started_at/1000, 'unixepoch')".to_string(),
        }
    }

    fn tz_hour_expr(params: &FilterParams) -> String {
        match params.tz_offset {
            Some(tz) if tz != 0 => {
                let sign = if tz > 0 { "+" } else { "" };
                format!("strftime('%H:00', m.started_at/1000, 'unixepoch', '{}{} hours')", sign, tz)
            }
            _ => "strftime('%H:00', m.started_at/1000, 'unixepoch')".to_string(),
        }
    }

    /// 构建动态 WHERE 子句。
    /// aliased=true 时字段带 `m.` 前缀（用于聚合查询的表别名）。
    /// 时间绑定值需秒→毫秒（×1000），因为 ZCode started_at 为毫秒。
    fn build_where_clause(
        params: &FilterParams,
        aliased: bool,
    ) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let prefix = if aliased { "m." } else { "" };
        let mut clauses: Vec<String> = vec![
            "1=1".to_string(),
            format!("{}status = 'completed'", prefix),
            format!(
                "({prefix}input_tokens > 0 OR {prefix}output_tokens > 0 OR {prefix}cache_read_input_tokens > 0 OR {prefix}cache_creation_input_tokens > 0)",
                prefix = prefix
            ),
        ];
        let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(from_epoch) = params.from_epoch {
            if from_epoch > 0 {
                clauses.push(format!("{}started_at >= ?", prefix));
                binds.push(Box::new(from_epoch * 1000));
            }
        }
        if let Some(to_epoch) = params.to_epoch {
            if to_epoch > 0 {
                clauses.push(format!("{}started_at < ?", prefix));
                binds.push(Box::new(to_epoch * 1000));
            }
        }
        if let Some(ref provider_id) = params.provider_id {
            if !provider_id.is_empty() && provider_id != "ZCode" {
                // 统一为单一 provider "ZCode"，其他 provider_id 无法匹配
                clauses.push("0 = 1".to_string());
            }
        }
        if let Some(ref model_id) = params.model_id {
            if !model_id.is_empty() {
                clauses.push(format!("{}model_id = ?", prefix));
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
                "SELECT COUNT(*) FROM model_usage WHERE status = 'completed'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询记录数失败: {}", e))?;
        Ok(count)
    }

    fn get_latest_timestamp_internal(&self) -> Option<i64> {
        self.db().ok().and_then(|db| {
            db.query_row(
                "SELECT MAX(started_at)/1000 FROM model_usage WHERE status = 'completed'",
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
        // ZCode 数据库无 providers 表，统一返回单一 provider
        Ok(vec![Provider {
            id: "ZCode".to_string(),
            name: "ZCode".to_string(),
        }])
    }

    pub fn get_models(&self) -> Result<Vec<String>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare(
                "SELECT DISTINCT model_id FROM model_usage WHERE status = 'completed' ORDER BY model_id",
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
                "SELECT MIN(started_at)/1000, MAX(started_at)/1000
                 FROM model_usage WHERE status = 'completed'",
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
                SUM(input_tokens - cache_read_input_tokens) AS total_input,
                SUM(output_tokens) AS total_output,
                SUM(cache_read_input_tokens) AS total_cache_read,
                SUM(cache_creation_input_tokens) AS total_cache_creation,
                ROUND(AVG(duration_ms), 0) AS avg_latency
             FROM model_usage {}",
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
                m.model_id,
                COUNT(*) AS requests,
                SUM(m.input_tokens - m.cache_read_input_tokens) AS input_tokens,
                SUM(m.output_tokens) AS output_tokens,
                SUM(m.cache_read_input_tokens) AS cache_read,
                SUM(m.cache_creation_input_tokens) AS cache_creation
             FROM model_usage m
             {}
             GROUP BY m.model_id
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
        // ZCode 无 providers 表，统一显示 provider 为 "ZCode"
        let sql = format!(
            "SELECT
                'ZCode' AS provider_name,
                'ZCode' AS provider_id,
                COUNT(*) AS requests,
                COUNT(*) AS successes,
                100.0 AS success_rate,
                ROUND(AVG(m.duration_ms), 0) AS avg_latency
             FROM model_usage m
             {}
             GROUP BY m.provider_id
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
                'ZCode' AS provider_id,
                m.model_id,
                COUNT(*) AS requests,
                SUM(m.input_tokens - m.cache_read_input_tokens) AS input_tokens,
                SUM(m.output_tokens) AS output_tokens,
                SUM(m.cache_read_input_tokens) AS cache_read,
                SUM(m.cache_creation_input_tokens) AS cache_creation,
                COALESCE(SUM(m.duration_ms), 0) AS latency_sum
             FROM model_usage m
             {}
             GROUP BY day, m.model_id
             ORDER BY day, m.model_id",
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
                'ZCode' AS provider_id,
                m.model_id,
                SUM(m.input_tokens - m.cache_read_input_tokens) AS input_tokens,
                SUM(m.output_tokens) AS output_tokens,
                SUM(m.cache_read_input_tokens) AS cache_read,
                SUM(m.cache_creation_input_tokens) AS cache_creation
             FROM model_usage m
             {}
             GROUP BY m.model_id",
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
        let day_expr = Self::tz_date_expr(params);
        let sql = format!(
            "SELECT
                {} AS day,
                m.model_id,
                COUNT(*) AS requests,
                SUM(m.input_tokens - m.cache_read_input_tokens) AS input_tokens,
                SUM(m.output_tokens) AS output_tokens,
                SUM(m.cache_read_input_tokens) AS cache_read,
                SUM(m.cache_creation_input_tokens) AS cache_creation,
                ROUND(AVG(m.duration_ms), 0) AS avg_latency
             FROM model_usage m
             {}
             GROUP BY day, m.model_id
             ORDER BY day",
            day_expr, where_sql
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
                m.model_id,
                COUNT(*) AS requests,
                SUM(m.input_tokens - m.cache_read_input_tokens) AS input_tokens,
                SUM(m.output_tokens) AS output_tokens,
                SUM(m.cache_read_input_tokens) AS cache_read,
                SUM(m.cache_creation_input_tokens) AS cache_creation,
                ROUND(AVG(m.duration_ms), 0) AS avg_latency
             FROM model_usage m
             {}
             GROUP BY day, m.model_id
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

    /// ZCode 请求不参与会话归类，直接返回空。
    /// 与 claude-desktop 处理一致：会话 tab 不展示 ZCode 会话，
    /// 请求仅在实时 tab 按 provider "ZCode" 聚合展示。
    pub fn get_session_breakdown(&self, _params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> {
        Ok(Vec::new())
    }

    pub fn get_session_max_context_widths(&self, session_ids: &[String]) -> Result<HashMap<String, i64>, String> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let db = self.db()?;
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        // ZCode input_tokens 为 cache-inclusive 口径，本身就是完整上下文（含 cache_read）
        let sql = format!(
            "SELECT session_id,
                    MAX(input_tokens) AS max_ctx
             FROM model_usage
             WHERE session_id IN ({})
               AND input_tokens > 0
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
                m.session_id,
                m.model_id,
                SUM(m.input_tokens - m.cache_read_input_tokens) AS input_tokens,
                SUM(m.output_tokens) AS output_tokens,
                SUM(m.cache_read_input_tokens) AS cache_read,
                SUM(m.cache_creation_input_tokens) AS cache_creation
             FROM model_usage m
             {}
               AND m.session_id IS NOT NULL AND m.session_id != ''
               AND m.session_id IN (
                   SELECT s.session_id FROM model_usage s
                   {}
                     AND s.session_id IS NOT NULL AND s.session_id != ''
                   GROUP BY s.session_id ORDER BY COUNT(*) DESC LIMIT {}
               )
             GROUP BY m.session_id, m.model_id",
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
                m.session_id,
                m.model_id,
                'ZCode' AS provider_id,
                m.started_at/1000 AS started_at,
                m.input_tokens,
                m.output_tokens,
                m.cache_read_input_tokens,
                m.cache_creation_input_tokens
             FROM model_usage m
             {}
               AND m.session_id IS NOT NULL AND m.session_id != ''
               AND m.session_id IN (
                   SELECT s.session_id FROM model_usage s
                   {}
                     AND s.session_id IS NOT NULL AND s.session_id != ''
                   GROUP BY s.session_id ORDER BY COUNT(*) DESC LIMIT {}
               )
             ORDER BY m.session_id, m.started_at",
            where_sql, sub_where, SESSION_TOP_N
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话请求Token失败: {}", e))?;
        let mut all_binds: Vec<Box<dyn rusqlite::types::ToSql>> = binds;
        all_binds.extend(sub_binds);
        let refs: Vec<&dyn rusqlite::types::ToSql> = all_binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
                let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
                Ok(SessionRequestToken {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    provider_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    input_tokens: fresh_input(raw_input, cache_read),
                    output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cache_read,
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
                m.session_id,
                m.model_id,
                'ZCode' AS provider_id,
                m.started_at/1000 AS started_at,
                m.input_tokens,
                m.output_tokens,
                m.cache_read_input_tokens,
                m.cache_creation_input_tokens
             FROM model_usage m
             {}
               AND m.session_id IN ({})
             ORDER BY m.session_id, m.started_at",
            where_sql, placeholders.join(",")
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询会话请求Token失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
                let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
                Ok(SessionRequestToken {
                    session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    provider_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    input_tokens: fresh_input(raw_input, cache_read),
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
                m.session_id,
                m.model_id,
                SUM(m.input_tokens - m.cache_read_input_tokens) AS input_tokens,
                SUM(m.output_tokens) AS output_tokens,
                SUM(m.cache_read_input_tokens) AS cache_read,
                SUM(m.cache_creation_input_tokens) AS cache_creation
             FROM model_usage m
             {}
               AND m.session_id IN ({})
             GROUP BY m.session_id, m.model_id",
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
        // ZCode input_tokens 为 cache-inclusive 口径，本身就是完整上下文
        let mut case_expr = String::from("CASE");
        for &th in tier_thresholds.iter().rev() {
            case_expr.push_str(&format!(
                " WHEN m.input_tokens >= {} THEN {}",
                th, th
            ));
        }
        case_expr.push_str(" ELSE 0 END");

        let sql = format!(
            "SELECT
                m.model_id,
                {} AS day,
                {} AS context_tier,
                SUM(m.input_tokens - m.cache_read_input_tokens) AS input_tokens,
                SUM(m.output_tokens) AS output_tokens,
                SUM(m.cache_read_input_tokens) AS cache_read,
                SUM(m.cache_creation_input_tokens) AS cache_creation,
                MIN(m.started_at)/1000 AS representative_epoch
             FROM model_usage m
             {}
             GROUP BY m.model_id, day, context_tier",
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
                    slot_key: -1,
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
            "SELECT session_id, started_at/1000 AS started_at FROM model_usage
             WHERE session_id IN ({})
             ORDER BY session_id, started_at",
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
        // started_at 毫秒，绑定值 ×1000；bucket 按 10 秒分桶（秒级）
        let sql = "
            SELECT (started_at/1000 / 10) * 10 AS bucket,
                   COUNT(*) AS requests,
                   SUM(input_tokens - cache_read_input_tokens) AS input_tokens,
                   SUM(output_tokens) AS output_tokens,
                   SUM(cache_read_input_tokens) AS cache_read,
                   SUM(cache_creation_input_tokens) AS cache_creation
            FROM model_usage
            WHERE started_at >= ?
              AND status = 'completed'
              AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_input_tokens > 0 OR cache_creation_input_tokens > 0)
            GROUP BY bucket
            ORDER BY bucket";

        let mut stmt = db.prepare(sql).map_err(|e| format!("查询实时趋势失败: {}", e))?;
        let rows = stmt
            .query_map(params![one_hour_ago * 1000], |row| {
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
        // 增量游标契约：返回 created_at（started_at/1000 截断秒）>= since 的记录。
        // 过滤必须在 SQL 内同样截断到秒：若用 started_at > s*1000 毫秒精确比较，
        // 已见记录的毫秒尾数会让它永远满足条件，每轮轮询重复返回。
        // >= 会重复返回游标秒内已见记录，由前端增量合并去重兜底。
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match since {
            Some(s) => ("
                SELECT session_id, model_id, 'ZCode', started_at/1000,
                       input_tokens, output_tokens,
                       cache_read_input_tokens, cache_creation_input_tokens,
                       duration_ms
                FROM model_usage
                WHERE (started_at / 1000) >= ?
                  AND status = 'completed'
                  AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_input_tokens > 0 OR cache_creation_input_tokens > 0)
                ORDER BY started_at DESC", vec![Box::new(s)]),
            None => ("
                SELECT session_id, model_id, 'ZCode', started_at/1000,
                       input_tokens, output_tokens,
                       cache_read_input_tokens, cache_creation_input_tokens,
                       duration_ms
                FROM model_usage
                WHERE status = 'completed'
                  AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_input_tokens > 0 OR cache_creation_input_tokens > 0)
                ORDER BY started_at DESC
                LIMIT 500", vec![]),
        };

        let mut stmt = db.prepare(sql).map_err(|e| format!("查询最近请求日志失败: {}", e))?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
            Ok((
                String::new(), // ZCode 不参与会话归类，session_id 置空
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                fresh_input(raw_input, cache_read),
                row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                cache_read,
                row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                false, // ZCode 非 Codex 协议
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
        // 增量游标契约：返回 created_at（started_at/1000 截断秒）>= since 的记录。
        // 过滤必须在 SQL 内同样截断到秒：若用 started_at > s*1000 毫秒精确比较，
        // 已见记录的毫秒尾数会让它永远满足条件，每轮轮询重复返回。
        // >= 会重复返回游标秒内已见记录，由前端增量合并去重兜底。
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match since {
            Some(s) => ("
                SELECT session_id, model_id, 'ZCode', started_at/1000,
                       input_tokens, output_tokens,
                       cache_read_input_tokens, cache_creation_input_tokens,
                       duration_ms
                FROM model_usage
                WHERE (started_at / 1000) >= ?
                  AND status = 'completed'
                  AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_input_tokens > 0 OR cache_creation_input_tokens > 0)
                ORDER BY started_at DESC", vec![Box::new(s)]),
            None => ("
                SELECT session_id, model_id, 'ZCode', started_at/1000,
                       input_tokens, output_tokens,
                       cache_read_input_tokens, cache_creation_input_tokens,
                       duration_ms
                FROM model_usage
                WHERE status = 'completed'
                  AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_input_tokens > 0 OR cache_creation_input_tokens > 0)
                ORDER BY started_at DESC
                LIMIT 500", vec![]),
        };

        let mut stmt = db.prepare(sql).map_err(|e| format!("stream_records 准备失败: {}", e))?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut rows = stmt.query_map(params_refs.as_slice(), |row| {
            let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
            Ok((
                String::new(), // ZCode 不参与会话归类，session_id 置空
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                fresh_input(raw_input, cache_read),
                row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                cache_read,
                row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                false,
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
            "SELECT session_id, model_id, 'ZCode', started_at/1000,
                    input_tokens, output_tokens,
                    cache_read_input_tokens, cache_creation_input_tokens,
                    duration_ms
             FROM model_usage {}
             ORDER BY started_at",
            where_sql
        );

        let mut stmt = db.prepare(&sql).map_err(|e| format!("查询过滤记录失败: {}", e))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            let raw_input: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let cache_read: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
            Ok(RawRecord {
                session_id: String::new(), // ZCode 不参与会话归类，session_id 置空
                model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                provider_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                db_type: "ZCode".to_string(),
                created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                input_tokens: fresh_input(raw_input, cache_read),
                output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                cache_read,
                cache_creation: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                latency: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                is_codex: false,
            })
        }).map_err(|e| format!("查询过滤记录失败: {}", e))?;

        Self::collect_rows(rows, "读取过滤记录")
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
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        }).map_err(|e| format!("查询会话标题失败: {}", e))?;

        let mut result = HashMap::new();
        for row in rows {
            let (sid, title, directory) = row.map_err(|e| format!("读取会话标题失败: {}", e))?;
            result.insert(sid, (title, directory));
        }
        Ok(result)
    }
}

impl super::data_source::DataSource for ZCodeDbService {
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
    fn title_source_tag(&self) -> Option<&'static str> { Some("zcode") }
    fn get_session_titles_from_provider(
        &self,
        session_ids: &[String],
    ) -> Option<Result<HashMap<String, (String, String)>, String>> {
        Some(self.get_session_titles_from_db(session_ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建含最小 model_usage 表的临时库（毫秒时间戳），先写后只读打开（与 dsh_db 测试同模式）
    fn temp_zcode_db() -> String {
        let path = std::env::temp_dir().join(format!(
            "ccsa_zcode_db_test_{}_{}.db",
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
        let insert = |conn: &Connection, id: &str, started_at: i64, input: i64, cache_read: i64| {
            conn.execute(
                "INSERT INTO model_usage (id, session_id, model_id, status, started_at, duration_ms,
                                          input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens)
                 VALUES (?1, 'sess', 'glm-5', 'completed', ?2, 1000, ?3, 100, ?4, 0)",
                params![id, started_at, input, cache_read],
            )
            .unwrap();
        };
        // 同一秒 1000：毫秒尾数非 0（a1）与恰好为 0（a2）各一条，尾数不应影响游标过滤
        insert(&conn, "a1", 1_000_500, 200, 0);
        insert(&conn, "a2", 1_000_000, 350, 100);
        insert(&conn, "b1", 2_000_700, 500, 400);
        drop(conn);
        path.to_string_lossy().to_string()
    }

    #[test]
    fn recent_request_logs_raw_cursor_semantics_inclusive_second() {
        let path_str = temp_zcode_db();
        let mut svc = ZCodeDbService::new();
        svc.open(&path_str).unwrap();

        // 全量：started_at 倒序
        let all = svc.get_recent_request_logs_raw(None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].3, 2000);
        assert_eq!(all[2].3, 1000);

        // 游标 = 记录自身所在秒：>= 语义必须返回该记录（毫秒尾数不参与过滤）
        let at_cursor = svc.get_recent_request_logs_raw(Some(2000)).unwrap();
        assert_eq!(at_cursor.len(), 1);
        assert_eq!(at_cursor[0].3, 2000);

        // 游标秒内同秒两条都返回（含毫秒尾数恰为 0 的 a2），更新的秒也一并返回
        let second_1000 = svc.get_recent_request_logs_raw(Some(1000)).unwrap();
        assert_eq!(second_1000.len(), 3);
        assert_eq!(second_1000[0].3, 2000);
        assert_eq!(second_1000[1].3, 1000);
        assert_eq!(second_1000[2].3, 1000);

        // 游标越过：更早记录不再返回
        assert!(svc.get_recent_request_logs_raw(Some(2001)).unwrap().is_empty());

        // input 为 cache-inclusive 口径，读取时归一化为 fresh input
        assert_eq!(at_cursor[0].4, 100); // b1: 500 - 400
        let a2 = second_1000.iter().find(|r| r.4 == 250).unwrap();
        assert_eq!(a2.4, 250); // a2: 350 - 100
    }

    #[test]
    fn stream_records_applies_same_cursor_semantics() {
        let path_str = temp_zcode_db();
        let mut svc = ZCodeDbService::new();
        svc.open(&path_str).unwrap();

        let mut collected = Vec::new();
        svc.stream_records(Some(2000), &mut |r| collected.push(r)).unwrap();
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].3, 2000);
        // ZCode 不参与会话归类，session_id 置空
        assert_eq!(collected[0].0, "");
        assert_eq!(collected[0].2, "ZCode");
    }
}
