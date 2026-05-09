use serde::{Deserialize, Serialize};

// ========== 查询参数 ==========

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterParams {
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

// ========== 数据库元信息 ==========

#[derive(Debug, Clone, Serialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DateRange {
    pub min: i64,
    pub max: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInfo {
    pub path: String,
    pub record_count: i64,
    pub date_range: DateRange,
    pub providers: Vec<Provider>,
    pub models: Vec<String>,
}

// ========== 基础定价 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub model_id: String,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_creation_cost_per_million: f64,
}

// ========== 汇总统计 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryData {
    pub total_requests: i64,
    pub success_count: i64,
    pub total_input: i64,
    pub total_output: i64,
    pub total_cache_read: i64,
    pub total_cache_creation: i64,
    pub avg_latency: f64,
}

// ========== 模型/供应商分组 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelBreakdown {
    pub model: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBreakdown {
    pub provider_name: String,
    pub provider_id: String,
    pub requests: i64,
    pub successes: i64,
    pub success_rate: f64,
    pub avg_latency: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelToken {
    pub provider_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyTrendRow {
    pub day: String,
    pub model: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub avg_latency: f64,
}

// ========== 实时趋势 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeBucket {
    pub bucket: i64,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeRequestLog {
    pub session_id: String,
    pub model: String,
    pub provider_id: String,
    pub created_at: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub latency_ms: i64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: f64,
    pub cache_creation_cost: f64,
    pub total_cost: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_tier_threshold: Option<i64>,
}

// ========== 会话分析 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBreakdown {
    pub session_id: String,
    pub requests: i64,
    pub max_context_width: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub first_at: i64,
    pub last_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelToken {
    pub session_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRequestToken {
    pub session_id: String,
    pub model: String,
    pub created_at: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

// ========== 缓存窗口 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheWindow {
    pub start_ts: i64,
    pub end_ts: i64,
    pub duration_sec: i64,
    pub hits: i64,
}

// ========== 定价相关 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextTier {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub threshold: i64,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_creation_cost_per_million: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedPricing {
    pub model_id: String,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_creation_cost_per_million: f64,
    pub is_override: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingOverride {
    pub model_id: String,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_creation_cost_per_million: f64,
    pub updated_at: i64,
    #[serde(default)]
    pub context_tiers: Vec<ContextTier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimePricingRule {
    pub id: i64,
    pub model_id: String,
    pub start_time: i64,
    pub end_time: i64,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_creation_cost_per_million: f64,
    pub label: String,
    #[serde(default)]
    pub context_tiers: Vec<ContextTier>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingData {
    pub model_id: String,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_creation_cost_per_million: f64,
    pub is_override: bool,
    pub has_time_pricing: bool,
    pub time_rules: Vec<TimePricingRule>,
    pub is_used: bool,
    #[serde(default)]
    pub context_tiers: Vec<ContextTier>,
    #[serde(default)]
    pub cloud_time_rules: Vec<CloudPricingTimeRule>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

// ========== 预计算结果 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecomputedResult {
    pub model_costs: std::collections::HashMap<String, f64>,
    pub model_cost_breakdown: std::collections::HashMap<String, Vec<f64>>,
    pub provider_costs: std::collections::HashMap<String, f64>,
    pub day_cost_map: std::collections::HashMap<String, f64>,
    pub day_requests_map: std::collections::HashMap<String, i64>,
    pub day_input_tokens: std::collections::HashMap<String, i64>,
    pub day_output_tokens: std::collections::HashMap<String, i64>,
    pub day_latency_sum: std::collections::HashMap<String, f64>,
    pub day_latency_count: std::collections::HashMap<String, i64>,
    pub daily_by_model: std::collections::HashMap<String, Vec<DailyTrendRow>>,
    pub cache_durations: std::collections::HashMap<String, i64>,
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub model_context_tier_costs: std::collections::HashMap<String, Vec<ContextTierCost>>,
}

// ========== 组合查询结果 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecomputeQueryResult {
    pub summary: SummaryData,
    pub model_breakdown: Vec<ModelBreakdown>,
    pub provider_breakdown: Vec<ProviderBreakdown>,
    pub precomputed: PrecomputedResult,
}

// ========== 会话费用数据 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextTierCost {
    pub threshold: i64,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelCostEntry {
    pub session_id: String,
    pub model: String,
    pub cost: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: f64,
    pub cache_creation_cost: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_tier_costs: Vec<ContextTierCost>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWithCost {
    pub session_id: String,
    pub request_count: i64,
    pub total_tokens: i64,
    pub max_context_width: i64,
    pub start_time: i64,
    pub end_time: i64,
    pub cache_hit_rate: f64,
    pub total_cost: f64,
    pub duration_sec: i64,
    pub timestamps: Vec<i64>,
    pub model_breakdown: Vec<SessionModelCostEntry>,
}

// ========== 刷新检测结果 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub has_new: bool,
    pub record_count: Option<i64>,
}

// ========== 云端定价 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudPricingTimeRule {
    #[serde(default)]
    pub model_id: String,
    pub label: String,
    pub start_time: i64,
    pub end_time: i64,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_creation_cost_per_million: f64,
    #[serde(default)]
    pub context_tiers: Vec<ContextTier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudPricingModel {
    pub model_id: String,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_creation_cost_per_million: f64,
    #[serde(default)]
    pub context_tiers: Vec<ContextTier>,
    #[serde(default)]
    pub time_rules: Vec<CloudPricingTimeRule>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudPricingData {
    pub version: i64,
    pub updated_at: i64,
    pub currency: String,
    pub models: Vec<CloudPricingModel>,
}

// ========== 筛选选项 ==========

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions {
    pub providers: Vec<Provider>,
    pub models: Vec<String>,
    pub date_range: DateRange,
}
