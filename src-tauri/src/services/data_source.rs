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

pub fn create_data_source(db_type: &DbType) -> Box<dyn DataSource> {
    match db_type {
        DbType::ExternalDb => Box::new(super::external_db::ExternalDbService::new()),
        DbType::OpenCode => Box::new(super::opencode_db::OpenCodeDbService::new()),
    }
}
