use std::collections::HashMap;
use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::services::data_source::*;
use crate::services::precompute::*;

macro_rules! require_sources {
    ($sources:expr) => {
        if $sources.is_empty() {
            return Err("未加载数据库".to_string());
        }
    };
}

macro_rules! collect_from_sources {
    ($sources:expr, $entry:ident, $method:ident($($arg:expr),*), $label:expr) => {
        $sources.iter().filter_map(|$entry| {
            $entry.source.$method($($arg),*).map_err(|e| {
                log::warn!("[QUERY] {} 数据源({}) 查询失败: {}", $label, $entry.db_type.label(), e);
                e
            }).ok()
        }).collect()
    };
}

#[tauri::command]
pub fn query_summary(params: FilterParams, state: State<AppState>) -> Result<SummaryData, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let results: Vec<SummaryData> = collect_from_sources!(sources, e, get_summary(&params), "summary");
    Ok(merge_summaries(results))
}

#[tauri::command]
pub fn query_by_model(params: FilterParams, state: State<AppState>) -> Result<Vec<ModelBreakdown>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let results: Vec<Vec<ModelBreakdown>> = collect_from_sources!(sources, e, get_model_breakdown(&params), "model_breakdown");
    Ok(merge_model_breakdowns(results))
}

#[tauri::command]
pub fn query_by_provider(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderBreakdown>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let results: Vec<Vec<ProviderBreakdown>> = collect_from_sources!(sources, e, get_provider_breakdown(&params), "provider_breakdown");
    Ok(merge_provider_breakdowns(results))
}

#[tauri::command]
pub fn query_provider_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderModelToken>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let results: Vec<Vec<ProviderModelToken>> = collect_from_sources!(sources, e, get_provider_model_tokens(&params), "provider_model_tokens");
    Ok(merge_provider_model_tokens(results))
}

#[tauri::command]
pub fn query_daily_trend(params: FilterParams, state: State<AppState>) -> Result<Vec<DailyTrendRow>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let results: Vec<Vec<DailyTrendRow>> = collect_from_sources!(sources, e, get_daily_trend(&params), "daily_trend");
    Ok(merge_daily_trends(results))
}

#[tauri::command]
pub fn query_sessions(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionBreakdown>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let mut all = Vec::new();
    for entry in sources.iter() {
        match entry.source.get_session_breakdown(&params) {
            Ok(sessions) => all.extend(sessions),
            Err(e) => log::warn!("[QUERY] session_breakdown 数据源({}) 查询失败: {}", entry.db_type.label(), e),
        }
    }
    all.sort_by(|a, b| b.requests.cmp(&a.requests));
    all.truncate(crate::utils::SESSION_TOP_N as usize);
    Ok(all)
}

#[tauri::command]
pub fn query_session_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionModelToken>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let mut all = Vec::new();
    for entry in sources.iter() {
        match entry.source.get_session_model_tokens(&params) {
            Ok(tokens) => all.extend(tokens),
            Err(e) => log::warn!("[QUERY] session_model_tokens 数据源({}) 查询失败: {}", entry.db_type.label(), e),
        }
    }
    Ok(all)
}

#[tauri::command]
pub fn query_session_request_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionRequestToken>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let mut all = Vec::new();
    for entry in sources.iter() {
        match entry.source.get_session_request_tokens(&params) {
            Ok(tokens) => all.extend(tokens),
            Err(e) => log::warn!("[QUERY] session_request_tokens 数据源({}) 查询失败: {}", entry.db_type.label(), e),
        }
    }
    Ok(all)
}

#[tauri::command]
pub fn query_session_timestamps(session_ids: Vec<String>, state: State<AppState>) -> Result<HashMap<String, Vec<i64>>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let mut result = HashMap::new();
    for entry in sources.iter() {
        match entry.source.get_session_timestamps(&session_ids) {
            Ok(map) => {
                for (k, v) in map {
                    result.entry(k).or_insert_with(Vec::new).extend(v);
                }
            }
            Err(e) => log::warn!("[QUERY] session_timestamps 数据源({}) 查询失败: {}", entry.db_type.label(), e),
        }
    }
    Ok(result)
}

#[tauri::command]
pub fn query_realtime(state: State<AppState>) -> Result<Vec<RealtimeBucket>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let results: Vec<Vec<RealtimeBucket>> = collect_from_sources!(sources, e, get_minute_level_token_trend(), "realtime");
    Ok(merge_realtime_buckets(results))
}

#[tauri::command]
pub fn query_realtime_logs(since: Option<i64>, state: State<AppState>) -> Result<Vec<RealtimeRequestLog>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let mut all_raw = Vec::new();
    for entry in sources.iter() {
        match entry.source.get_recent_request_logs_raw(since) {
            Ok(raw) => all_raw.extend(raw),
            Err(e) => log::warn!("[QUERY] recent_request_logs 数据源({}) 查询失败: {}", entry.db_type.label(), e),
        }
    }
    drop(sources);

    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;

    let result: Vec<RealtimeRequestLog> = all_raw.into_iter().map(|(session_id, model, provider_id, created_at, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, latency_ms)| {
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
            session_id, model, provider_id, created_at,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            latency_ms, input_cost, output_cost, cache_read_cost, cache_creation_cost,
            total_cost: input_cost + output_cost + cache_read_cost + cache_creation_cost,
            context_tier_threshold,
        }
    }).collect();

    Ok(result)
}

#[tauri::command]
pub fn query_precompute(params: FilterParams, state: State<AppState>) -> Result<PrecomputeQueryResult, String> {
    log::debug!("[QUERY] query_precompute: params={:?}", params);

    let (summary, provider_breakdown, combined, tier_buckets) = {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        require_sources!(sources);

        let summaries: Vec<SummaryData> = collect_from_sources!(sources, e, get_summary(&params), "summary");
        let provider_results: Vec<Vec<ProviderBreakdown>> = collect_from_sources!(sources, e, get_provider_breakdown(&params), "provider_breakdown");
        let combined_results: Vec<Vec<CombinedBreakdownRow>> = collect_from_sources!(sources, e, get_combined_breakdown(&params), "combined_breakdown");

        let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
        let thresholds = pricing.get_all_tier_thresholds();
        drop(pricing);

        let tier_results: Vec<Vec<ModelContextTierBucket>> = if thresholds.is_empty() {
            Vec::new()
        } else {
            collect_from_sources!(sources, e, get_model_context_tier_buckets(&params, &thresholds), "tier_buckets")
        };

        let s = merge_summaries(summaries);
        let pb = merge_provider_breakdowns(provider_results);
        let cb = merge_combined(combined_results);
        let tb: Vec<ModelContextTierBucket> = {
            let mut map: HashMap<(String, String, i64), ModelContextTierBucket> = HashMap::new();
            for list in tier_results {
                for row in list {
                    let key = (row.model.clone(), row.day.clone(), row.context_tier);
                    map.entry(key)
                        .and_modify(|e| {
                            e.input_tokens += row.input_tokens;
                            e.output_tokens += row.output_tokens;
                            e.cache_read += row.cache_read;
                            e.cache_creation += row.cache_creation;
                        })
                        .or_insert(row);
                }
            }
            map.into_values().collect()
        };

        log::debug!("[QUERY] summary.requests={}, providers={}, combined_rows={}, tier_buckets={}",
            s.total_requests, pb.len(), cb.len(), tb.len());
        (s, pb, cb, tb)
    };

    let agg = aggregate_combined_breakdown(&combined);

    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    log::debug!("[QUERY] 定价引擎模型数={}", pricing.size());

    let tz_offset = params.tz_offset.unwrap_or(0);
    let mut precomputed = precompute_costs(&agg.daily_trend, &agg.provider_model_tokens, &pricing, tz_offset);

    let (tier_costs, ctx_model_costs, ctx_model_breakdown) = build_context_tier_and_model_costs(&tier_buckets, &pricing);
    precomputed.model_context_tier_costs = tier_costs;
    if !ctx_model_costs.is_empty() {
        precomputed.model_costs = ctx_model_costs;
        precomputed.model_cost_breakdown = ctx_model_breakdown;

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
    let (sessions, max_context_widths, session_request_tokens, session_model_tokens, timestamps_map) = {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        require_sources!(sources);

        let mut all_sessions = Vec::new();
        let mut all_request_tokens = Vec::new();
        let mut all_model_tokens = Vec::new();

        for entry in sources.iter() {
            if let Ok(s) = entry.source.get_session_breakdown(&params) {
                all_sessions.extend(s);
            }
            if let Ok(r) = entry.source.get_session_request_tokens(&params) {
                all_request_tokens.extend(r);
            }
            if let Ok(m) = entry.source.get_session_model_tokens(&params) {
                all_model_tokens.extend(m);
            }
        }

        all_sessions.sort_by(|a, b| b.requests.cmp(&a.requests));
        let top_session_ids: Vec<String> = all_sessions.iter().take(20).map(|s| s.session_id.clone()).collect();

        let mut max_ctx = HashMap::new();
        let mut ts_map = HashMap::new();
        for entry in sources.iter() {
            if let Ok(m) = entry.source.get_session_max_context_widths(&top_session_ids) {
                for (k, v) in m { max_ctx.insert(k, v); }
            }
            if let Ok(t) = entry.source.get_session_timestamps(&top_session_ids) {
                for (k, v) in t { ts_map.entry(k).or_insert_with(Vec::new).extend(v); }
            }
        }

        (all_sessions, max_ctx, all_request_tokens, all_model_tokens, ts_map)
    };

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
