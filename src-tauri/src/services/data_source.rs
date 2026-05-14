use std::collections::HashMap;

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
