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

#[tauri::command]
pub fn query_summary(params: FilterParams, state: State<AppState>) -> Result<SummaryData, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_summary(&params)
}

#[tauri::command]
pub fn query_by_model(params: FilterParams, state: State<AppState>) -> Result<Vec<ModelBreakdown>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_model_breakdown(&params)
}

#[tauri::command]
pub fn query_by_provider(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderBreakdown>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_provider_breakdown(&params)
}

#[tauri::command]
pub fn query_provider_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderModelToken>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_provider_model_tokens(&params)
}

#[tauri::command]
pub fn query_daily_trend(params: FilterParams, state: State<AppState>) -> Result<Vec<DailyTrendRow>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_daily_trend(&params)
}

#[tauri::command]
pub fn query_sessions(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionBreakdown>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_session_breakdown(&params)
}

#[tauri::command]
pub fn query_session_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionModelToken>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_session_model_tokens(&params)
}

#[tauri::command]
pub fn query_session_request_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionRequestToken>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_session_request_tokens(&params)
}

#[tauri::command]
pub fn query_session_timestamps(session_ids: Vec<String>, state: State<AppState>) -> Result<HashMap<String, Vec<i64>>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_session_timestamps(&session_ids)
}

#[tauri::command]
pub fn query_realtime(state: State<AppState>) -> Result<Vec<RealtimeBucket>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    ext_db.get_minute_level_token_trend()
}

#[tauri::command]
pub fn query_realtime_logs(since: Option<i64>, state: State<AppState>) -> Result<Vec<RealtimeRequestLog>, String> {
    let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    let raw = ext_db.get_recent_request_logs_raw(since)?;
    drop(ext_db);

    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;

    let result: Vec<RealtimeRequestLog> = raw.into_iter().map(|(session_id, model, provider_id, created_at, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, latency_ms)| {
        let context_size = input_tokens + cache_read_tokens;
        let (input_cost, output_cost, cache_read_cost, cache_creation_cost) =
            if let Some(p) = pricing.get_pricing_at_with_context(&model, created_at, context_size) {
                (
                    input_tokens as f64 * p.input_cost_per_million / 1_000_000.0,
                    output_tokens as f64 * p.output_cost_per_million / 1_000_000.0,
                    cache_read_tokens as f64 * p.cache_read_cost_per_million / 1_000_000.0,
                    cache_creation_tokens as f64 * p.cache_creation_cost_per_million / 1_000_000.0,
                )
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };
        let context_tier_threshold = pricing.get_matched_tier_threshold(&model, created_at, context_size);
        RealtimeRequestLog {
            session_id,
            model,
            provider_id,
            created_at,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            latency_ms,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost: input_cost + output_cost + cache_read_cost + cache_creation_cost,
            context_tier_threshold,
        }
    }).collect();

    Ok(result)
}

#[tauri::command]
pub fn query_precompute(params: FilterParams, state: State<AppState>) -> Result<PrecomputeQueryResult, String> {
    log::debug!("[QUERY] query_precompute: params={:?}", params);

    // Phase 1: DB queries only
    let (summary, provider_breakdown, combined, tier_buckets) = {
        let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
        require_db!(ext_db);
        let s = ext_db.get_summary(&params)?;
        let pb = ext_db.get_provider_breakdown(&params)?;
        let cb = ext_db.get_combined_breakdown(&params)?;

        // 获取 tier 阈值用于 SQL 端聚合
        let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
        let thresholds = pricing.get_all_tier_thresholds();
        drop(pricing);
        let tb = if thresholds.is_empty() {
            Vec::new()
        } else {
            ext_db.get_model_context_tier_buckets(&params, &thresholds)?
        };

        log::debug!("[QUERY] summary.requests={}, providers={}, combined_rows={}, tier_buckets={}", s.total_requests, pb.len(), cb.len(), tb.len());
        (s, pb, cb, tb)
    }; // ext_db lock dropped

    // 从合并查询结果派生 model_breakdown, daily_trend, provider_model_tokens
    let agg = aggregate_combined_breakdown(&combined);

    // Phase 2: Pricing computation only
    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    log::debug!("[QUERY] 定价引擎模型数={}", pricing.size());

    let tz_offset = params.tz_offset.unwrap_or(0);
    let mut precomputed = precompute_costs(&agg.daily_trend, &agg.provider_model_tokens, &pricing, tz_offset);

    // Per-model 上下文档位计费比例 + 上下文感知模型费用（替代非上下文感知的 precompute_costs 结果）
    let (tier_costs, ctx_model_costs, ctx_model_breakdown) = build_context_tier_and_model_costs(&tier_buckets, &pricing);
    precomputed.model_context_tier_costs = tier_costs;
    if !ctx_model_costs.is_empty() {
        // 用上下文感知的费用覆盖非上下文感知的结果
        precomputed.model_costs = ctx_model_costs;
        precomputed.model_cost_breakdown = ctx_model_breakdown;

        // 重新按 Token 比例分配供应商费用
        precomputed.provider_costs.clear();
        let mut model_total_tokens: HashMap<String, i64> = HashMap::new();
        for pmt in &agg.provider_model_tokens {
            let total = pmt.input_tokens + pmt.output_tokens + pmt.cache_read + pmt.cache_creation;
            *model_total_tokens.entry(pmt.model.clone()).or_insert(0) += total;
        }
        for pmt in &agg.provider_model_tokens {
            let model_cost = precomputed.model_costs.get(&pmt.model).copied().unwrap_or(0.0);
            if model_cost <= 0.0 { continue; }
            let pmt_tokens = pmt.input_tokens + pmt.output_tokens + pmt.cache_read + pmt.cache_creation;
            let total_tokens = model_total_tokens.get(&pmt.model).copied().unwrap_or(0);
            if total_tokens <= 0 || pmt_tokens <= 0 { continue; }
            *precomputed.provider_costs.entry(pmt.provider_id.clone()).or_insert(0.0) +=
                model_cost * (pmt_tokens as f64 / total_tokens as f64);
        }
    }

    Ok(PrecomputeQueryResult {
        summary,
        model_breakdown: agg.model_breakdown,
        provider_breakdown,
        precomputed,
    })
}

#[tauri::command]
pub fn query_sessions_with_cost(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionWithCost>, String> {
    // Phase 1: DB queries only
    let (sessions, max_context_widths, session_request_tokens, session_model_tokens, timestamps_map) = {
        let ext_db = state.external_db.read().map_err(|e| e.to_string())?;
        require_db!(ext_db);
        let sessions = ext_db.get_session_breakdown(&params)?;
        let top_session_ids: Vec<String> = sessions.iter().take(20).map(|s| s.session_id.clone()).collect();
        let max_context_widths = ext_db.get_session_max_context_widths(&top_session_ids)?;
        let timestamps_map = ext_db.get_session_timestamps(&top_session_ids)?;
        let session_request_tokens = ext_db.get_session_request_tokens(&params)?;
        let session_model_tokens = ext_db.get_session_model_tokens(&params)?;
        (sessions, max_context_widths, session_request_tokens, session_model_tokens, timestamps_map)
    }; // ext_db lock dropped

    // Phase 2: Pricing computation only
    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;

    let session_costs = compute_session_costs(&session_request_tokens, &pricing);
    let session_model_costs = compute_session_model_costs(&session_request_tokens, &session_model_tokens, &pricing);

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
                        .map(|(model, data)| {
                            // 将 tier_costs HashMap 转为排序后的 Vec，过滤零值
                            let mut tier_vec: Vec<ContextTierCost> = data
                                .tier_costs
                                .iter()
                                .filter(|(_, c)| **c > 0.0)
                                .map(|(threshold, cost)| ContextTierCost {
                                    threshold: *threshold,
                                    cost: *cost,
                                    tokens: data.tier_tokens.get(threshold).copied().unwrap_or(0),
                                })
                                .collect();
                            tier_vec.sort_by_key(|t| t.threshold);

                            SessionModelCostEntry {
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
                                context_tier_costs: tier_vec,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();

            SessionWithCost {
                session_id: s.session_id.clone(),
                request_count: s.requests,
                total_tokens: s.input_tokens + s.output_tokens + s.cache_read + s.cache_creation,
                max_context_width: max_context_widths.get(&s.session_id).copied().unwrap_or(0),
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
