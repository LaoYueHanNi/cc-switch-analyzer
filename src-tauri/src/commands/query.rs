use std::collections::HashMap;
use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::services::precompute::*;

macro_rules! require_db {
    ($ext_db:expr) => {
        if !$ext_db.is_open() {
            return Err("数据库未打开".to_string());
        }
    };
}

macro_rules! require_pricing {
    ($pricing:expr) => {
        if $pricing.size() == 0 {
            return Err("定价引擎未初始化".to_string());
        }
    };
}

#[tauri::command]
pub fn query_summary(params: FilterParams, state: State<AppState>) -> Result<SummaryData, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_summary(&params)
}

#[tauri::command]
pub fn query_by_model(params: FilterParams, state: State<AppState>) -> Result<Vec<ModelBreakdown>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_model_breakdown(&params)
}

#[tauri::command]
pub fn query_by_provider(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderBreakdown>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_provider_breakdown(&params)
}

#[tauri::command]
pub fn query_provider_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderModelToken>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_provider_model_tokens(&params)
}

#[tauri::command]
pub fn query_daily_trend(params: FilterParams, state: State<AppState>) -> Result<Vec<DailyTrendRow>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_daily_trend(&params)
}

#[tauri::command]
pub fn query_cache_durations(params: FilterParams, state: State<AppState>) -> Result<HashMap<String, i64>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_cache_non_decay_duration(&params)
}

#[tauri::command]
pub fn query_cache_windows(model_id: String, state: State<AppState>) -> Result<Vec<CacheWindow>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_recent_cache_windows(&model_id)
}

#[tauri::command]
pub fn query_sessions(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionBreakdown>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_session_breakdown(&params)
}

#[tauri::command]
pub fn query_session_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionModelToken>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_session_model_tokens(&params)
}

#[tauri::command]
pub fn query_session_request_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionRequestToken>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_session_request_tokens(&params)
}

#[tauri::command]
pub fn query_session_timestamps(session_ids: Vec<String>, state: State<AppState>) -> Result<HashMap<String, Vec<i64>>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_session_timestamps(&session_ids)
}

#[tauri::command]
pub fn query_realtime(state: State<AppState>) -> Result<Vec<RealtimeBucket>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_minute_level_token_trend()
}

#[tauri::command]
pub fn query_precompute(params: FilterParams, state: State<AppState>) -> Result<PrecomputeQueryResult, String> {
    eprintln!("[QUERY] query_precompute: params={:?}", params);
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    let pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;
    eprintln!("[QUERY] 定价引擎模型数={}", pricing.size());

    let summary = ext_db.get_summary(&params)?;
    let model_breakdown = ext_db.get_model_breakdown(&params)?;
    let provider_breakdown = ext_db.get_provider_breakdown(&params)?;
    let daily_trend = ext_db.get_daily_trend(&params)?;
    let provider_model_tokens = ext_db.get_provider_model_tokens(&params)?;
    let cache_durations = ext_db.get_cache_non_decay_duration(&params)?;
    eprintln!("[QUERY] summary.requests={}, models={}, providers={}, days={}", summary.total_requests, model_breakdown.len(), provider_breakdown.len(), daily_trend.len());

    let mut precomputed = precompute_costs(&daily_trend, &provider_model_tokens, &pricing);
    precomputed.cache_durations = cache_durations;

    Ok(PrecomputeQueryResult {
        summary,
        model_breakdown,
        provider_breakdown,
        precomputed,
    })
}

#[tauri::command]
pub fn query_sessions_with_cost(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionWithCost>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    let pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;

    let sessions = ext_db.get_session_breakdown(&params)?;
    let session_request_tokens = ext_db.get_session_request_tokens(&params)?;
    let session_model_tokens = ext_db.get_session_model_tokens(&params)?;

    let session_costs = compute_session_costs(&session_request_tokens, &pricing);
    let session_model_costs = compute_session_model_costs(&session_request_tokens, &session_model_tokens, &pricing);

    let top_session_ids: Vec<String> = sessions.iter().take(20).map(|s| s.session_id.clone()).collect();
    let timestamps_map = ext_db.get_session_timestamps(&top_session_ids)?;

    let enriched: Vec<SessionWithCost> = sessions
        .iter()
        .take(20)
        .map(|s| {
            let cost = session_costs.get(&s.session_id).copied().unwrap_or(0.0);
            let model_costs = session_model_costs.get(&s.session_id);
            let timestamps = timestamps_map.get(&s.session_id).cloned().unwrap_or_default();
            let duration_sec = s.last_at - s.first_at;
            let cache_hit_rate = if (s.input_tokens + s.cache_read) > 0 {
                s.cache_read as f64 / (s.input_tokens + s.cache_read) as f64
            } else {
                0.0
            };

            let model_breakdown: Vec<SessionModelCostEntry> = model_costs
                .map(|mc| {
                    mc.iter()
                        .map(|(model, data)| SessionModelCostEntry {
                            session_id: s.session_id.clone(),
                            model: model.clone(),
                            cost: data.cost,
                            input_tokens: data.input_tokens,
                            output_tokens: data.output_tokens,
                            cache_read_tokens: data.cache_read,
                            cache_creation_tokens: data.cache_creation,
                            input_cost: data.breakdown[0],
                            output_cost: data.breakdown[1],
                            cache_read_cost: data.breakdown[2],
                            cache_creation_cost: data.breakdown[3],
                        })
                        .collect()
                })
                .unwrap_or_default();

            SessionWithCost {
                session_id: s.session_id.clone(),
                request_count: s.requests,
                total_tokens: s.input_tokens + s.output_tokens + s.cache_read + s.cache_creation,
                max_context_width: s.max_context_width,
                start_time: s.first_at,
                end_time: s.last_at,
                cache_hit_rate,
                total_cost: cost,
                duration_sec,
                timestamps,
                model_breakdown,
            }
        })
        .collect();

    Ok(enriched)
}
