use std::collections::HashMap;

use rusqlite::{Connection, OpenFlags};
use std::path::Path;

use crate::models::*;

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
    fn get_session_breakdown(&self, params: &FilterParams) -> Result<Vec<SessionBreakdown>, String>;
    fn get_session_max_context_widths(&self, ids: &[String]) -> Result<HashMap<String, i64>, String>;
    fn get_session_model_tokens(&self, params: &FilterParams) -> Result<Vec<SessionModelToken>, String>;
    fn get_session_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String>;
    fn get_session_timestamps(&self, ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String>;
    fn get_model_context_tier_buckets(&self, params: &FilterParams, thresholds: &[i64]) -> Result<Vec<ModelContextTierBucket>, String>;
    fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String>;
    fn get_recent_request_logs_raw(&self, since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64)>, String>;
}

#[derive(Debug)]
pub enum DbType {
    ExternalDb,
    OpenCode,
}

impl DbType {
    pub fn label(&self) -> &'static str {
        match self {
            DbType::ExternalDb => "CC-Switch",
            DbType::OpenCode => "OpenCode",
        }
    }
}

pub struct SourceEntry {
    pub id: String,
    pub path: String,
    pub db_type: DbType,
    pub source: Box<dyn DataSource>,
}

impl SourceEntry {
    pub fn to_info(&self) -> SourceInfo {
        SourceInfo {
            id: self.id.clone(),
            path: self.path.clone(),
            db_type: self.db_type.label().to_string(),
            record_count: self.source.get_record_count().unwrap_or(0),
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

    Err("无法识别数据库类型".to_string())
}

pub fn create_source_entry(path: &str) -> Result<SourceEntry, String> {
    let db_type = detect_db_type(path)?;
    let id = format!("{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis());
    let mut source = match &db_type {
        DbType::ExternalDb => Box::new(super::external_db::ExternalDbService::new()) as Box<dyn DataSource>,
        DbType::OpenCode => Box::new(super::opencode_db::OpenCodeDbService::new()) as Box<dyn DataSource>,
    };
    source.open(path)?;
    Ok(SourceEntry { id, path: path.to_string(), db_type, source })
}

// ========== 合并函数 ==========

pub fn merge_summaries(results: Vec<SummaryData>) -> SummaryData {
    if results.is_empty() {
        return SummaryData {
            total_requests: 0, success_count: 0,
            total_input: 0, total_output: 0,
            total_cache_read: 0, total_cache_creation: 0,
            avg_latency: 0.0,
        };
    }
    if results.len() == 1 { return results.into_iter().next().unwrap(); }

    let total_requests: i64 = results.iter().map(|r| r.total_requests).sum();
    let weighted_latency: f64 = results.iter()
        .map(|r| r.avg_latency * r.total_requests as f64)
        .sum();

    SummaryData {
        total_requests,
        success_count: results.iter().map(|r| r.success_count).sum(),
        total_input: results.iter().map(|r| r.total_input).sum(),
        total_output: results.iter().map(|r| r.total_output).sum(),
        total_cache_read: results.iter().map(|r| r.total_cache_read).sum(),
        total_cache_creation: results.iter().map(|r| r.total_cache_creation).sum(),
        avg_latency: if total_requests > 0 { weighted_latency / total_requests as f64 } else { 0.0 },
    }
}

pub fn merge_model_breakdowns(results: Vec<Vec<ModelBreakdown>>) -> Vec<ModelBreakdown> {
    let mut map: HashMap<String, ModelBreakdown> = HashMap::new();
    for list in results {
        for mb in list {
            map.entry(mb.model.clone())
                .and_modify(|e| {
                    e.requests += mb.requests;
                    e.input_tokens += mb.input_tokens;
                    e.output_tokens += mb.output_tokens;
                    e.cache_read += mb.cache_read;
                    e.cache_creation += mb.cache_creation;
                })
                .or_insert(mb);
        }
    }
    let mut v: Vec<_> = map.into_values().collect();
    v.sort_by(|a, b| b.requests.cmp(&a.requests));
    v
}

pub fn merge_provider_breakdowns(results: Vec<Vec<ProviderBreakdown>>) -> Vec<ProviderBreakdown> {
    let mut map: HashMap<String, ProviderBreakdown> = HashMap::new();
    for list in results {
        for pb in list {
            map.entry(pb.provider_id.clone())
                .and_modify(|e| {
                    e.requests += pb.requests;
                    e.successes += pb.successes;
                })
                .or_insert(pb);
        }
    }
    let mut v: Vec<_> = map.into_values().map(|mut pb| {
        pb.success_rate = if pb.requests > 0 { 100.0 * pb.successes as f64 / pb.requests as f64 } else { 0.0 };
        pb
    }).collect();
    v.sort_by(|a, b| b.requests.cmp(&a.requests));
    v
}

pub fn merge_combined(results: Vec<Vec<CombinedBreakdownRow>>) -> Vec<CombinedBreakdownRow> {
    let mut map: HashMap<(String, String, String), CombinedBreakdownRow> = HashMap::new();
    for list in results {
        for row in list {
            let key = (row.day.clone(), row.provider_id.clone(), row.model.clone());
            map.entry(key)
                .and_modify(|e| {
                    e.requests += row.requests;
                    e.input_tokens += row.input_tokens;
                    e.output_tokens += row.output_tokens;
                    e.cache_read += row.cache_read;
                    e.cache_creation += row.cache_creation;
                    e.latency_sum += row.latency_sum;
                })
                .or_insert(row);
        }
    }
    let mut v: Vec<_> = map.into_values().collect();
    v.sort_by(|a, b| (&a.day, &a.provider_id, &a.model).cmp(&(&b.day, &b.provider_id, &b.model)));
    v
}

pub fn merge_provider_model_tokens(results: Vec<Vec<ProviderModelToken>>) -> Vec<ProviderModelToken> {
    let mut map: HashMap<(String, String), ProviderModelToken> = HashMap::new();
    for list in results {
        for pmt in list {
            let key = (pmt.provider_id.clone(), pmt.model.clone());
            map.entry(key)
                .and_modify(|e| {
                    e.input_tokens += pmt.input_tokens;
                    e.output_tokens += pmt.output_tokens;
                    e.cache_read += pmt.cache_read;
                    e.cache_creation += pmt.cache_creation;
                })
                .or_insert(pmt);
        }
    }
    map.into_values().collect()
}

pub fn merge_daily_trends(results: Vec<Vec<DailyTrendRow>>) -> Vec<DailyTrendRow> {
    let mut map: HashMap<(String, String), DailyTrendRow> = HashMap::new();
    for list in results {
        for row in list {
            let key = (row.day.clone(), row.model.clone());
            map.entry(key)
                .and_modify(|e| {
                    e.requests += row.requests;
                    e.input_tokens += row.input_tokens;
                    e.output_tokens += row.output_tokens;
                    e.cache_read += row.cache_read;
                    e.cache_creation += row.cache_creation;
                    // avg_latency 加权（后续重算）
                })
                .or_insert(row);
        }
    }
    let mut v: Vec<_> = map.into_values().collect();
    v.sort_by(|a, b| (&a.day, &a.model).cmp(&(&b.day, &b.model)));
    v
}

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
