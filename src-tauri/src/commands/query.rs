use std::collections::HashMap;
use tauri::State;

use crate::AppState;
use crate::models::{self, *};
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
pub fn query_realtime_logs(since: Option<i64>, state: State<AppState>) -> Result<Vec<RealtimeRequestLog>, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    require_db!(ext_db);
    let raw = ext_db.get_recent_request_logs_raw(since)?;
    drop(ext_db);

    let pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;

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
    eprintln!("[QUERY] query_precompute: params={:?}", params);

    // Phase 1: DB queries only
    let (summary, provider_breakdown, combined, tier_buckets) = {
        let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
        require_db!(ext_db);
        let s = ext_db.get_summary(&params)?;
        let pb = ext_db.get_provider_breakdown(&params)?;
        let cb = ext_db.get_combined_breakdown(&params)?;

        // 获取 tier 阈值用于 SQL 端聚合
        let pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;
        let thresholds = pricing.get_all_tier_thresholds();
        drop(pricing);
        let tb = if thresholds.is_empty() {
            Vec::new()
        } else {
            ext_db.get_model_context_tier_buckets(&params, &thresholds)?
        };

        eprintln!("[QUERY] summary.requests={}, providers={}, combined_rows={}, tier_buckets={}", s.total_requests, pb.len(), cb.len(), tb.len());
        (s, pb, cb, tb)
    }; // ext_db lock dropped

    // 从合并查询结果派生 model_breakdown, daily_trend, provider_model_tokens
    let mut model_map: HashMap<String, (i64, i64, i64, i64, i64)> = HashMap::new(); // model -> (requests, input, output, cache_read, cache_creation)
    let mut daily_map: HashMap<(String, String), (i64, i64, i64, i64, i64, f64)> = HashMap::new(); // (day, model) -> (requests, input, output, cache_read, cache_creation, latency_sum)
    let mut pmt_map: HashMap<(String, String), (i64, i64, i64, i64)> = HashMap::new(); // (provider_id, model) -> (input, output, cache_read, cache_creation)

    for row in &combined {
        // model_breakdown 聚合
        let e = model_map.entry(row.model.clone()).or_insert((0, 0, 0, 0, 0));
        e.0 += row.requests;
        e.1 += row.input_tokens;
        e.2 += row.output_tokens;
        e.3 += row.cache_read;
        e.4 += row.cache_creation;

        // daily_trend 聚合
        let dt_key = (row.day.clone(), row.model.clone());
        let dt = daily_map.entry(dt_key).or_insert((0, 0, 0, 0, 0, 0.0));
        dt.0 += row.requests;
        dt.1 += row.input_tokens;
        dt.2 += row.output_tokens;
        dt.3 += row.cache_read;
        dt.4 += row.cache_creation;
        dt.5 += row.latency_sum;

        // provider_model_tokens 聚合
        let pmt_key = (row.provider_id.clone(), row.model.clone());
        let pmt = pmt_map.entry(pmt_key).or_insert((0, 0, 0, 0));
        pmt.0 += row.input_tokens;
        pmt.1 += row.output_tokens;
        pmt.2 += row.cache_read;
        pmt.3 += row.cache_creation;
    }

    let mut model_breakdown: Vec<models::ModelBreakdown> = model_map
        .into_iter()
        .map(|(model, (requests, input_tokens, output_tokens, cache_read, cache_creation))| {
            models::ModelBreakdown { model, requests, input_tokens, output_tokens, cache_read, cache_creation }
        })
        .collect();
    model_breakdown.sort_by(|a, b| b.requests.cmp(&a.requests));

    let mut daily_trend: Vec<DailyTrendRow> = daily_map
        .into_iter()
        .map(|((day, model), (requests, input_tokens, output_tokens, cache_read, cache_creation, latency_sum))| {
            DailyTrendRow {
                day,
                model,
                requests,
                input_tokens,
                output_tokens,
                cache_read,
                cache_creation,
                avg_latency: if requests > 0 { latency_sum / requests as f64 } else { 0.0 },
            }
        })
        .collect();
    daily_trend.sort_by(|a, b| a.day.cmp(&b.day).then(a.model.cmp(&b.model)));

    let provider_model_tokens: Vec<models::ProviderModelToken> = pmt_map
        .into_iter()
        .map(|((provider_id, model), (input_tokens, output_tokens, cache_read, cache_creation))| {
            models::ProviderModelToken { provider_id, model, input_tokens, output_tokens, cache_read, cache_creation }
        })
        .collect();

    // Phase 2: Pricing computation only
    let pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;
    eprintln!("[QUERY] 定价引擎模型数={}", pricing.size());

    let tz_offset = params.tz_offset.unwrap_or(0);
    let mut precomputed = precompute_costs(&daily_trend, &provider_model_tokens, &pricing, tz_offset);

    // Per-model 上下文档位计费比例
    let model_tiers = compute_model_context_tier_costs_from_buckets(&tier_buckets, &pricing);
    let model_tier_costs: HashMap<String, Vec<ContextTierCost>> = model_tiers
        .into_iter()
        .map(|(model, tiers)| {
            let mut vec: Vec<ContextTierCost> = tiers
                .into_iter()
                .filter(|(_, c)| *c > 0.0)
                .map(|(threshold, cost)| ContextTierCost { threshold, cost })
                .collect();
            vec.sort_by_key(|t| t.threshold);
            (model, vec)
        })
        .collect();
    precomputed.model_context_tier_costs = model_tier_costs;

    Ok(PrecomputeQueryResult {
        summary,
        model_breakdown,
        provider_breakdown,
        precomputed,
    })
}

#[tauri::command]
pub fn query_sessions_with_cost(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionWithCost>, String> {
    // Phase 1: DB queries only
    let (sessions, max_context_widths, session_request_tokens, session_model_tokens, timestamps_map) = {
        let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
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
    let pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;

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
