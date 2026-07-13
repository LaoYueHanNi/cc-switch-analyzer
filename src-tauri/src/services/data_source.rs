use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OpenFlags};
use std::path::Path;

use crate::models::*;

static SOURCE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub trait DataSource: Send + Sync {
    fn open(&mut self, path: &str) -> Result<(), String>;
    fn close(&mut self);
    fn is_open(&self) -> bool;

    fn get_record_count(&self) -> Result<i64, String>;
    fn get_latest_timestamp(&self) -> Option<i64>;
    fn get_providers(&self) -> Result<Vec<Provider>, String>;
    fn get_models(&self) -> Result<Vec<String>, String>;
    fn get_date_range(&self) -> Result<DateRange, String>;

    fn get_summary(&self, params: &FilterParams) -> Result<SummaryData, String>;
    fn get_model_breakdown(&self, params: &FilterParams) -> Result<Vec<ModelBreakdown>, String>;
    fn get_provider_breakdown(&self, params: &FilterParams) -> Result<Vec<ProviderBreakdown>, String>;
    fn get_combined_breakdown(&self, params: &FilterParams) -> Result<Vec<CombinedBreakdownRow>, String>;
    fn get_provider_model_tokens(&self, params: &FilterParams) -> Result<Vec<ProviderModelToken>, String>;
    fn get_daily_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String>;
    fn get_hourly_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String>;
    fn get_session_breakdown(&self, params: &FilterParams) -> Result<Vec<SessionBreakdown>, String>;
    fn get_session_max_context_widths(&self, ids: &[String]) -> Result<HashMap<String, i64>, String>;
    fn get_session_model_tokens(&self, params: &FilterParams) -> Result<Vec<SessionModelToken>, String>;
    fn get_session_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String>;
    fn get_session_request_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionRequestToken>, String>;
    fn get_session_model_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionModelToken>, String>;
    fn get_session_timestamps(&self, ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String>;
    fn get_model_context_tier_buckets(&self, params: &FilterParams, thresholds: &[i64]) -> Result<Vec<ModelContextTierBucket>, String>;
    fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String>;
    fn get_recent_request_logs_raw(&self, since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64, bool)>, String>;

    /// 按 FilterParams 过滤查询原始请求记录，返回 RawRecord。
    /// 这是去重管道的入口：所有聚合查询应从这里取数据。
    fn get_filtered_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String>;

    /// 流式查询请求记录。逐行通过 callback 发射，避免一次性加载全部数据到内存。
    /// 默认实现委托给 `get_recent_request_logs_raw`。
    /// Tuple 末尾 `is_codex` 标记该记录是否来自 OpenAI Codex 协议（用于会话重映射）。
    fn stream_records(
        &self,
        since: Option<i64>,
        on_record: &mut dyn FnMut((String, String, String, i64, i64, i64, i64, i64, i64, bool)),
    ) -> Result<(), String> {
        for record in self.get_recent_request_logs_raw(since)? {
            on_record(record);
        }
        Ok(())
    }

    /// 数据源提供的标题来源标签，用于 UI 展示。
    /// CC-Switch 返回 None（标题由 JSONL 或其他 provider 解析），OpenCode 返回 "opencode"。
    fn title_source_tag(&self) -> Option<&'static str> { None }

    /// 从数据源获取会话标题和项目名。默认不提供。
    /// 返回 HashMap<session_id, (title, project_name)>
    fn get_session_titles_from_provider(
        &self,
        _session_ids: &[String],
    ) -> Option<Result<HashMap<String, (String, String)>, String>> {
        None
    }

    /// Cursor 本机归因 token 统计；非 Cursor 源返回 None。
    fn get_cursor_attribution_stats(
        &self,
    ) -> Option<crate::services::cursor_attribution::AttributionTokenStats> {
        None
    }
}

#[derive(Debug)]
pub enum DbType {
    ExternalDb,
    OpenCode,
    AiProxy,
    Cursor,
}

impl DbType {
    pub fn label(&self) -> &'static str {
        match self {
            DbType::ExternalDb => "CC-Switch",
            DbType::OpenCode => "OpenCode",
            DbType::AiProxy => "AI-Proxy",
            DbType::Cursor => "Cursor",
        }
    }
}

pub struct SourceEntry {
    pub id: String,
    pub path: String,
    pub db_type: DbType,
    pub source: Box<dyn DataSource>,
    pub enabled: bool,
}

impl SourceEntry {
    pub fn to_info(&self) -> SourceInfo {
        SourceInfo {
            id: self.id.clone(),
            path: self.path.clone(),
            db_type: self.db_type.label().to_string(),
            record_count: self.source.get_record_count().unwrap_or(0),
            enabled: self.enabled,
        }
    }
}

pub fn detect_db_type(path: &str) -> Result<DbType, String> {
    let conn = Connection::open_with_flags(
        Path::new(path),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("无法打开数据库: {}", e))?;

    let has_proxy_logs: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='proxy_request_logs'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if has_proxy_logs {
        return Ok(DbType::ExternalDb);
    }

    let has_message: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='message'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if has_message {
        return Ok(DbType::OpenCode);
    }

    let has_token_stats: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='token_stats'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if has_token_stats {
        return Ok(DbType::AiProxy);
    }

    Err("无法识别数据库类型".to_string())
}

pub fn create_source_entry(path: &str) -> Result<SourceEntry, String> {
    let path_obj = Path::new(path);
    if path_obj.is_dir() && super::cursor_csv::detect_cursor_cache(path) {
        let id = SOURCE_ID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string();
        let mut source = Box::new(super::cursor_csv::CursorCsvService::new()) as Box<dyn DataSource>;
        source.open(path)?;
        return Ok(SourceEntry {
            id,
            path: path.to_string(),
            db_type: DbType::Cursor,
            source,
            enabled: true,
        });
    }

    let db_type = detect_db_type(path)?;
    let id = SOURCE_ID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string();
    let mut source = match &db_type {
        DbType::ExternalDb => Box::new(super::external_db::ExternalDbService::new()) as Box<dyn DataSource>,
        DbType::OpenCode => Box::new(super::opencode_db::OpenCodeDbService::new()) as Box<dyn DataSource>,
        DbType::AiProxy => Box::new(super::ai_proxy_db::AiProxyDbService::new()) as Box<dyn DataSource>,
        DbType::Cursor => return Err("Cursor 数据源需使用缓存目录路径".to_string()),
    };
    source.open(path)?;
    Ok(SourceEntry { id, path: path.to_string(), db_type, source, enabled: true })
}

// ========== 合并函数（供 realtime 等仍用 per-source SQL 聚合的查询使用）==========

pub fn merge_realtime_buckets(results: Vec<Vec<RealtimeBucket>>) -> Vec<RealtimeBucket> {
    let mut map: HashMap<i64, RealtimeBucket> = HashMap::new();
    for list in results {
        for b in list {
            map.entry(b.bucket)
                .and_modify(|e| {
                    e.requests += b.requests;
                    e.input_tokens += b.input_tokens;
                    e.output_tokens += b.output_tokens;
                    e.cache_read += b.cache_read;
                    e.cache_creation += b.cache_creation;
                })
                .or_insert(b);
        }
    }
    let mut v: Vec<_> = map.into_values().collect();
    v.sort_by_key(|b| b.bucket);
    v
}

pub fn union_providers(results: Vec<Vec<Provider>>) -> Vec<Provider> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for list in results {
        for p in list {
            if seen.insert(p.id.clone()) {
                out.push(p);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn union_models(results: Vec<Vec<String>>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for list in results {
        for m in list {
            if seen.insert(m.clone()) {
                out.push(m);
            }
        }
    }
    out.sort();
    out
}

pub fn merge_date_range(ranges: Vec<DateRange>) -> DateRange {
    if ranges.is_empty() {
        return DateRange { min: 0, max: 0 };
    }
    DateRange {
        min: ranges.iter().map(|r| r.min).min().unwrap_or(0),
        max: ranges.iter().map(|r| r.max).max().unwrap_or(0),
    }
}
