use std::collections::HashMap;
use serde::Serialize;
use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::models::CompareBucket;
use crate::services::data_source::*;
use crate::services::precompute::*;
use crate::services::dedup::*;
use crate::services::pipeline::*;
use crate::services::session_title::{resolve_session_projects, find_jsonl_batch};

macro_rules! require_sources {
    ($sources:expr) => {
        if $sources.is_empty() {
            return Err("未加载数据库".to_string());
        }
    };
}

/// 并行线程 join，将 panic 转为日志而非进程崩溃。
fn join_scoped<T>(handle: std::thread::ScopedJoinHandle<'_, Option<T>>) -> Option<T> {
    match handle.join() {
        Ok(v) => v,
        Err(_) => {
            log::error!("[QUERY] 并行查询线程 panic");
            None
        }
    }
}

fn join_scoped_vec<T: Default>(handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
    match handle.join() {
        Ok(v) => v,
        Err(_) => {
            log::error!("[QUERY] 并行查询线程 panic");
            T::default()
        }
    }
}

fn join_scoped_flat<T>(handle: std::thread::ScopedJoinHandle<'_, Option<Vec<T>>>) -> Vec<T> {
    match handle.join() {
        Ok(Some(v)) => v,
        Ok(None) => Vec::new(),
        Err(_) => {
            log::error!("[QUERY] 并行查询线程 panic");
            Vec::new()
        }
    }
}

/// 并行查询辅助函数。调用者需预先 clone 参数。
/// `query` 接收 `&(entry, &cloned_params)` 元组，对每个数据源执行查询。
fn parallel_query<P, R, F>(
    sources: &[SourceEntry],
    params: P,
    label: &str,
    query: F,
) -> Vec<R>
where
    P: Clone + Sync + 'static,
    R: Send + 'static,
    F: for<'a> Fn(&'a SourceEntry, &'a P) -> Result<R, String> + Sync + Clone + Send,
{
    // 为每个 source 预 clone params，确保引用在 scope 内有效
    let params_cloned: Vec<_> = sources.iter().map(|_| params.clone()).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = sources.iter().zip(params_cloned.iter()).map(|(entry, p)| {
            let l = label.to_string();
            let q = query.clone();
            s.spawn(move || {
                q(entry, p).map_err(|e| {
                    log::warn!("[QUERY] {} 数据源({}) 查询失败: {}", l, entry.db_type.label(), e);
                    e
                }).ok()
            })
        }).collect();
        handles.into_iter().filter_map(join_scoped).collect()
    })
}


#[tauri::command]
pub fn query_summary(params: FilterParams, state: State<AppState>) -> Result<SummaryData, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_summary(&records))
}

#[tauri::command]
pub fn query_by_model(params: FilterParams, state: State<AppState>) -> Result<Vec<ModelBreakdown>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_model_breakdown(&records))
}

#[tauri::command]
pub fn query_by_provider(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderBreakdown>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    let provider_names = collect_provider_names(&sources);
    Ok(aggregate_provider_breakdown(&records, &provider_names))
}

#[tauri::command]
pub fn query_provider_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderModelToken>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_provider_model_tokens(&records))
}

#[tauri::command]
pub fn query_daily_trend(params: FilterParams, state: State<AppState>) -> Result<Vec<DailyTrendRow>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    let tz_offset = params.tz_offset.unwrap_or(0);
    Ok(aggregate_daily_trend(&records, tz_offset))
}

#[tauri::command]
pub fn query_hourly_trend(params: FilterParams, state: State<AppState>) -> Result<Vec<DailyTrendRow>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    let tz_offset = params.tz_offset.unwrap_or(0);
    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    let mut rows = aggregate_hourly_trend(&records, tz_offset);
    for row in &mut rows {
        if let Some(canonical) = pricing.resolve_model_id(&row.model) {
            row.model = canonical;
        }
    }
    Ok(rows)
}

#[tauri::command]
pub fn query_sessions(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionBreakdown>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_session_breakdown(&records))
}

#[tauri::command]
pub fn query_session_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionModelToken>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_session_model_tokens(&records))
}

#[tauri::command]
pub fn query_session_request_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionRequestToken>, String> {
    // 优先使用缓存（由 refresh_database 增量维护），按 params 过滤
    {
        let cache = state.request_cache.lock().map_err(|e| e.to_string())?;
        if cache.len() > 0 {
            let filtered: Vec<_> = cache.records().iter()
                .filter(|r| {
                    if let Some(from) = params.from_epoch {
                        if from > 0 && r.created_at < from { return false; }
                    }
                    if let Some(to) = params.to_epoch {
                        if to > 0 && r.created_at >= to { return false; }
                    }
                    if let Some(ref model) = params.model_id {
                        if !model.is_empty() && r.model != *model { return false; }
                    }
                    if let Some(ref provider) = params.provider_id {
                        if !provider.is_empty() && r.provider_id != *provider { return false; }
                    }
                    true
                })
                .cloned()
                .collect();
            return Ok(filtered);
        }
    }
    // 缓存为空，回退到全量并行查询 + 去重
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let all: Vec<Vec<SessionRequestToken>> = parallel_query(&sources, params.clone(), "session_request_tokens", |e, p| e.source.get_session_request_tokens(p));
    Ok(dedup_request_tokens(all.into_iter().flatten().collect()))
}

#[tauri::command]
pub fn query_session_timestamps(session_ids: Vec<String>, state: State<AppState>) -> Result<HashMap<String, Vec<i64>>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let all_maps: Vec<HashMap<String, Vec<i64>>> = {
        let srcs = &sources;
        std::thread::scope(|s| {
            let handles: Vec<_> = srcs.iter().map(|e| {
                let ids = session_ids.clone();
                s.spawn(move || {
                    let label = e.db_type.label();
                    e.source.get_session_timestamps(&ids).map_err(|e| {
                        log::warn!("[QUERY] session_timestamps 数据源({}) 查询失败: {}", label, e);
                        e
                    }).ok()
                })
            }).collect();
            handles.into_iter().filter_map(join_scoped).collect()
        })
    };
    let mut result = HashMap::new();
    for map in all_maps {
        for (k, v) in map {
            result.entry(k).or_insert_with(Vec::new).extend(v);
        }
    }
    Ok(result)
}

#[tauri::command]
pub fn query_realtime(state: State<AppState>) -> Result<Vec<RealtimeBucket>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let results: Vec<Vec<RealtimeBucket>> = parallel_query(&sources, (), "realtime", |e, _| e.source.get_minute_level_token_trend());
    Ok(merge_realtime_buckets(results))
}

#[tauri::command]
pub fn query_realtime_logs(since: Option<i64>, state: State<AppState>) -> Result<Vec<RealtimeRequestLog>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let all_raw = run_streaming_dedup(&sources, since);
    drop(sources);

    // Codex 会话重映射：CC-Switch 给 codex 每个请求分配独立 UUID 作为 session_id，
    // 通过时间戳匹配 codex JSONL 真实 session_id 把同一会话的多个请求合并。
    // 仅对 is_codex 记录做重映射，其他数据源 session_id 保持原值。
    let codex_ts_mapping = {
        let codex_ts: Vec<i64> = all_raw.iter()
            .filter_map(|(_, _, _, ts, _, _, _, _, _, is_codex)| is_codex.then_some(*ts))
            .collect();
        crate::services::codex_sessions::get_or_build_codex_ts_mapping(&codex_ts)
    };

    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    let tz_offset = (chrono::Local::now().offset().local_minus_utc() / 3600) as i64;

    let result: Vec<RealtimeRequestLog> = all_raw.into_iter().map(|(session_id, model, provider_id, created_at, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, latency_ms, is_codex)| {
        let session_id = if is_codex {
            codex_ts_mapping.get(&created_at).cloned().unwrap_or(session_id)
        } else {
            session_id
        };
        let context_size = input_tokens + cache_read_tokens;
        let (input_cost, output_cost, cache_read_cost, cache_creation_cost) =
            if let Some(p) = pricing.get_pricing_at_with_context(&model, created_at, context_size, tz_offset) {
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

/// 共享的预计算查询核心逻辑，不依赖 Tauri State，可被 HTTP 服务复用。
/// 使用中心去重管道：一次获取所有去重记录，在内存中聚合。
pub fn compute_precompute(
    sources: &[SourceEntry],
    pricing: &crate::services::pricing_engine::PricingEngine,
    params: &FilterParams,
) -> Result<PrecomputeQueryResult, String> {
    let tz_offset = params.tz_offset.unwrap_or(0);
    let thresholds = pricing.get_all_tier_thresholds();

    // 中心去重管道：一次获取所有去重记录
    let records = fetch_deduped_records(sources, params)?;

    // 内存聚合
    let summary = aggregate_summary(&records);
    let provider_names = collect_provider_names(sources);
    let provider_breakdown = aggregate_provider_breakdown(&records, &provider_names);
    let combined = aggregate_combined_records(&records, tz_offset);
    let tier_buckets = aggregate_model_context_tier_buckets(&records, tz_offset, &thresholds, Some(pricing));

    log::debug!("[QUERY] summary.requests={}, providers={}, combined_rows={}, tier_buckets={}",
        summary.total_requests, provider_breakdown.len(), combined.len(), tier_buckets.len());

    // 别名解析
    let combined = {
        let mut resolved = combined;
        for row in &mut resolved {
            if let Some(canonical) = pricing.resolve_model_id(&row.model) {
                row.model = canonical;
            }
        }
        resolved
    };
    let tier_buckets = {
        let mut resolved = tier_buckets;
        for bucket in &mut resolved {
            if let Some(canonical) = pricing.resolve_model_id(&bucket.model) {
                bucket.model = canonical;
            }
        }
        resolved
    };
    let agg = aggregate_combined_breakdown(&combined);

    let mut precomputed = precompute_costs(&agg.daily_trend, &agg.provider_model_tokens, pricing, tz_offset);

    let (tier_costs, ctx_model_costs, ctx_model_breakdown, ctx_day_cost_map) = build_context_tier_and_model_costs(&tier_buckets, pricing, tz_offset);
    precomputed.model_context_tier_costs = tier_costs;

    let mut compare_buckets_map: HashMap<String, Vec<CompareBucket>> = HashMap::new();
    for bucket in &tier_buckets {
        let actual_tier = pricing
            .get_matched_tier_threshold(&bucket.model, bucket.representative_epoch, bucket.context_tier.max(0))
            .unwrap_or(0);
        let cb = CompareBucket {
            threshold: actual_tier,
            representative_epoch: bucket.representative_epoch,
            input_tokens: bucket.input_tokens,
            output_tokens: bucket.output_tokens,
            cache_read: bucket.cache_read,
            cache_creation: bucket.cache_creation,
            slot_key: bucket.slot_key,
        };
        compare_buckets_map
            .entry(bucket.model.clone())
            .or_default()
            .push(cb);
    }
    precomputed.model_compare_buckets = compare_buckets_map;

    if !ctx_model_costs.is_empty() {
        precomputed.model_costs = ctx_model_costs;
        precomputed.model_cost_breakdown = ctx_model_breakdown;
        precomputed.day_cost_map = ctx_day_cost_map;

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
pub fn query_precompute(params: FilterParams, state: State<AppState>) -> Result<PrecomputeQueryResult, String> {
    log::debug!("[QUERY] query_precompute: params={:?}", params);

    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;

    compute_precompute(&sources, &pricing, &params)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsResponse {
    sessions: Vec<SessionWithCost>,
    available_projects: Vec<String>,
}

#[tauri::command]
pub fn query_sessions_with_cost(
    params: FilterParams,
    project: Option<String>,
    state: State<AppState>,
) -> Result<SessionsResponse, String> {
    // 第一轮：只查 session_breakdown
    let (all_sessions, session_sources) = {
        crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        require_sources!(sources);

        let session_lists: Vec<Vec<SessionBreakdown>> = std::thread::scope(|s| {
            let handles: Vec<_> = sources.iter().map(|entry| {
                let p = params.clone();
                let label = entry.db_type.label().to_string();
                s.spawn(move || {
                    entry.source.get_session_breakdown(&p).map_err(|e| {
                        log::warn!("[QUERY] session_breakdown 数据源({}) 查询失败: {}", label, e); e
                    }).ok().unwrap_or_default()
                })
            }).collect();
            handles.into_iter().map(join_scoped_vec).collect()
        });

        let mut session_sources: HashMap<String, Vec<String>> = HashMap::new();
        for (i, list) in session_lists.iter().enumerate() {
            let label = sources[i].db_type.label().to_string();
            for s in list {
                session_sources.entry(s.session_id.clone()).or_default().push(label.clone());
            }
        }
        let mut all_sessions: Vec<SessionBreakdown> = session_lists.into_iter().flatten().collect();
        all_sessions.sort_by(|a, b| b.requests.cmp(&a.requests));

        (all_sessions, session_sources)
    };

    // 查标题缓存 → 提取 available_projects + 过滤
    let all_ids: Vec<String> = all_sessions.iter().map(|s| s.session_id.clone()).collect();
    let cached_titles = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        app_db.get_session_titles(&all_ids)?
    };

    let available_projects: Vec<String> = {
        let mut projects: Vec<String> = cached_titles.values()
            .filter_map(|(raw, _)| {
                let parts: Vec<&str> = raw.splitn(2, '|').collect();
                let proj = parts.get(1).unwrap_or(&"");
                if !proj.is_empty() { Some(proj.to_string()) } else { None }
            })
            .collect();
        projects.sort();
        projects.dedup();
        projects
    };

    let filtered_sessions: Vec<&SessionBreakdown> = all_sessions.iter().filter(|s| {
        let cached = cached_titles.get(&s.session_id);
        // 有效性：requestCount <= 1 且无 source → 过滤
        if let Some((_, source)) = cached {
            if s.requests <= 1 && source.is_empty() {
                return false;
            }
        }
        // 目录筛选
        if let Some(ref proj) = project {
            if let Some((raw, _)) = cached {
                let parts: Vec<&str> = raw.splitn(2, '|').collect();
                return *parts.get(1).unwrap_or(&"") == proj.as_str();
            }
            return false;
        }
        true
    }).collect();

    let top_ids: Vec<String> = filtered_sessions.iter().take(20).map(|s| s.session_id.clone()).collect();

    // 第二轮 + 第三轮：只处理过滤后的 top 20
    let (session_request_tokens, session_model_tokens, max_context_widths, timestamps_map) = {
        crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        require_sources!(sources);

        let detail_data: Vec<_> = std::thread::scope(|s| {
            let handles: Vec<_> = sources.iter().map(|entry| {
                let p = params.clone();
                let ids = top_ids.clone();
                let label = entry.db_type.label().to_string();
                s.spawn(move || {
                    let req = entry.source.get_session_request_tokens_for_ids(&p, &ids).map_err(|e| {
                        log::warn!("[QUERY] session_request_tokens_for_ids 数据源({}) 查询失败: {}", label, e); e
                    }).ok();
                    let mdl = entry.source.get_session_model_tokens_for_ids(&p, &ids).map_err(|e| {
                        log::warn!("[QUERY] session_model_tokens_for_ids 数据源({}) 查询失败: {}", label, e); e
                    }).ok();
                    (req, mdl)
                })
            }).collect();
            handles.into_iter().map(join_scoped_vec).collect()
        });

        let all_request_tokens: Vec<SessionRequestToken> = detail_data.iter().filter_map(|r| r.0.clone()).flatten().collect();
        let all_model_tokens: Vec<SessionModelToken> = detail_data.iter().filter_map(|r| r.1.clone()).flatten().collect();

        let mut max_ctx = HashMap::new();
        let mut ts_map = HashMap::new();
        for entry in sources.iter() {
            if let Ok(m) = entry.source.get_session_max_context_widths(&top_ids) {
                for (k, v) in m { max_ctx.insert(k, v); }
            }
            if let Ok(t) = entry.source.get_session_timestamps(&top_ids) {
                for (k, v) in t { ts_map.entry(k).or_insert_with(Vec::new).extend(v); }
            }
        }

        (all_request_tokens, all_model_tokens, max_ctx, ts_map)
    };

    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    let tz_offset = params.tz_offset.unwrap_or(0);

    let session_costs = compute_session_costs(&session_request_tokens, &pricing, tz_offset);
    let session_model_costs = compute_session_model_costs(&session_request_tokens, &session_model_tokens, &pricing, tz_offset);

    let enriched: Vec<SessionWithCost> = filtered_sessions
        .iter()
        .take(20)
        .map(|s| {
            let cost = session_costs.get(&s.session_id).copied().unwrap_or(0.0);
            let model_costs = session_model_costs.get(&s.session_id);
            let timestamps = timestamps_map.get(&s.session_id).cloned().unwrap_or_default();
            let duration_sec = s.last_at - s.first_at;
            let cache_hit_rate = if (s.input_tokens + s.cache_read + s.cache_creation) > 0 {
                s.cache_read as f64 / (s.input_tokens + s.cache_read + s.cache_creation) as f64
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
                sources: session_sources.get(&s.session_id).cloned().unwrap_or_default(),
            }
        })
        .collect();

    Ok(SessionsResponse {
        sessions: enriched,
        available_projects,
    })
}

/// 会话管理 - 第一屏：按项目目录分组的聚合统计
#[tauri::command]
pub fn query_session_project_groups(
    params: FilterParams,
    state: State<AppState>,
) -> Result<Vec<ProjectGroupStats>, String> {
    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);

    // 1. 中心去重管道获取 session_breakdown
    let records = fetch_deduped_records(&sources, &params)?;
    let all_sessions = aggregate_session_breakdown(&records);

    if all_sessions.is_empty() { return Ok(vec![]); }

    // 1.5 Codex session 合并：CC-Switch 给每个请求分配独立 UUID，
    // 通过时间戳匹配到 Codex JSONL 的真实 session_id，合并同一会话的请求
    let codex_mapping = crate::services::codex_sessions::build_codex_session_mapping(&records);
    let all_ids: Vec<String> = all_sessions.iter().map(|s| s.session_id.clone()).collect();

    // 2. 解析项目目录（用原始 session_id 查）
    let (project_map, _) = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        resolve_session_projects(&all_ids, &app_db, &sources)?
    };

    // 2.5 获取 source_type 映射
    let source_type_map: HashMap<String, String> = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let cached = app_db.get_sessions(&all_ids)?;
        cached.into_iter()
            .filter_map(|(sid, (_, _, src))| if !src.is_empty() { Some((sid, src)) } else { None })
            .collect()
    };

    // 3. 计算基础费用（复用中心管道已去重的请求级 token，避免二次查库）
    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    let all_request_tokens: Vec<SessionRequestToken> = records.iter().map(|r| SessionRequestToken {
        session_id: r.session_id.clone(),
        model: r.model.clone(),
        provider_id: r.provider_id.clone(),
        created_at: r.created_at,
        input_tokens: r.input_tokens,
        output_tokens: r.output_tokens,
        cache_read: r.cache_read,
        cache_creation: r.cache_creation,
    }).collect();
    let tz_offset = params.tz_offset.unwrap_or(0);
    let session_costs = compute_session_costs(&all_request_tokens, &pricing, tz_offset);

    // 3.5 构建合并后的会话列表（按 codex_session_id 合并，保留原始 session_id 的 cost/project 映射）
    let merged_sessions = merge_codex_sessions(&all_sessions, &codex_mapping, &project_map, &source_type_map, &session_costs);

    // 4. 按 projectDir 分组聚合
    let mut groups: HashMap<String, (i64, f64, i64, i64, i64, Vec<String>, std::collections::HashSet<String>)> = HashMap::new();
    for s in &merged_sessions {
        let dir = &s.project_dir;
        let entry = groups.entry(dir.clone()).or_default();
        entry.0 += 1;
        entry.1 += s.total_cost;
        entry.2 += s.total_tokens;
        entry.3 = entry.3.min(s.first_at);
        entry.4 = entry.4.max(s.last_at);
        entry.5.push(s.session_id.clone());
        for st in &s.source_types {
            entry.6.insert(st.clone());
        }
    }

    // 5. 组装结果
    let mut result: Vec<ProjectGroupStats> = groups.into_iter().map(|(dir, (count, cost, tokens, first, last, session_ids, source_set))| {
        let display_name = if dir.is_empty() {
            "未知项目".to_string()
        } else {
            std::path::Path::new(&dir)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&dir)
                .to_string()
        };
        let mut source_types: Vec<String> = source_set.into_iter().collect();
        source_types.sort();
        ProjectGroupStats {
            project_dir: dir,
            display_name,
            session_count: count,
            total_cost: cost,
            total_tokens: tokens,
            first_at: first,
            last_at: last,
            session_ids,
            source_types,
        }
    }).collect();
    result.sort_by(|a, b| b.last_at.cmp(&a.last_at));
    Ok(result)
}

/// 会话管理 - 第二屏：按 sessionIds 直接加载详情
#[tauri::command]
pub fn query_project_session_details(
    params: FilterParams,
    session_ids: Vec<String>,
    state: State<AppState>,
) -> Result<Vec<ProjectSessionDetail>, String> {
    if session_ids.is_empty() { return Ok(vec![]); }

    crate::commands::cursor::sync_and_reload_if_needed(&state)?;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);

    // 0. 构建 Codex session 映射（合并 ID → 原始 ID 列表）
    let records = fetch_deduped_records(&sources, &params)?;
    let codex_fwd: HashMap<String, String> = crate::services::codex_sessions::build_codex_session_mapping(&records);
    let codex_rev = build_reverse_mapping(&codex_fwd);

    // 展开 Codex 合并 session_id 为原始 session_id
    let expanded_ids: Vec<String> = session_ids.iter().flat_map(|sid| {
        if let Some(originals) = codex_rev.get(sid) {
            originals.iter().cloned().collect()
        } else {
            vec![sid.clone()]
        }
    }).collect();
    let expanded_set: std::collections::HashSet<String> = expanded_ids.iter().cloned().collect();

    // 1. 从 sessions 表拿标题和目录（用展开后的 ID 查）
    let (title_map, project_map, source_type_map) = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let cached = app_db.get_sessions(&expanded_ids)?;
        let mut titles = HashMap::new();
        let mut projects = HashMap::new();
        let mut source_types = HashMap::new();
        for (sid, (dir, title, src)) in &cached {
            if !title.is_empty() { titles.insert(sid.clone(), title.clone()); }
            if !dir.is_empty() { projects.insert(sid.clone(), dir.clone()); }
            if !src.is_empty() { source_types.insert(sid.clone(), src.clone()); }
        }
        (titles, projects, source_types)
    };

    // 2. 过滤 session_breakdown（用展开后的 ID）
    let all_sessions: Vec<SessionBreakdown> = aggregate_session_breakdown(&records)
        .into_iter()
        .filter(|s| expanded_set.contains(&s.session_id))
        .collect();

    if all_sessions.is_empty() { return Ok(vec![]); }

    let filtered_ids: Vec<String> = all_sessions.iter().map(|s| s.session_id.clone()).collect();

    // 3. 加载完整数据（用原始 ID 查数据库）
    let (all_request_tokens, all_model_tokens, max_context_widths, timestamps_map) = {
        let all_req_raw: Vec<SessionRequestToken> = std::thread::scope(|s| {
            let handles: Vec<_> = sources.iter().map(|entry| {
                let p = params.clone();
                let ids = filtered_ids.clone();
                s.spawn(move || {
                    entry.source.get_session_request_tokens_for_ids(&p, &ids).ok()
                })
            }).collect();
            handles.into_iter().flat_map(join_scoped_flat).collect()
        });
        let all_req = dedup_request_tokens(all_req_raw);
        let all_mdl: Vec<SessionModelToken> = aggregate_session_model_tokens(&records)
            .into_iter()
            .filter(|t| expanded_set.contains(&t.session_id))
            .collect();

        let mut max_ctx: HashMap<String, i64> = HashMap::new();
        let mut ts_map: HashMap<String, Vec<i64>> = HashMap::new();
        for entry in sources.iter() {
            if let Ok(m) = entry.source.get_session_max_context_widths(&filtered_ids) {
                for (k, v) in m { max_ctx.entry(k).and_modify(|e| *e = (*e).max(v)).or_insert(v); }
            }
            if let Ok(t) = entry.source.get_session_timestamps(&filtered_ids) {
                for (k, v) in t { ts_map.entry(k).or_default().extend(v); }
            }
        }
        (all_req, all_mdl, max_ctx, ts_map)
    };

    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    let tz_offset = params.tz_offset.unwrap_or(0);
    let session_costs = compute_session_costs(&all_request_tokens, &pricing, tz_offset);
    let session_model_costs = compute_session_model_costs(&all_request_tokens, &all_model_tokens, &pricing, tz_offset);

    // 4. 批量构建 source_path
    let source_path_map: HashMap<String, String> = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let all_cached = app_db.get_sessions(&filtered_ids)?;
        let claude_ids: Vec<String> = filtered_ids.iter()
            .filter(|sid| all_cached.get(sid.as_str()).map(|(_, _, s)| s.as_str()) == Some("claudecode"))
            .cloned()
            .collect();
        find_jsonl_batch(&claude_ids)
    };

    // 5. 组装结果：Codex session 合并聚合
    let details = build_session_details(
        &all_sessions, &session_ids, &codex_fwd, &codex_rev,
        &session_costs, &session_model_costs, &max_context_widths, &timestamps_map,
        &title_map, &project_map, &source_type_map, &source_path_map,
    );

    Ok(details)
}

// ========== Codex session 合并 ==========

struct MergedSession {
    session_id: String,
    project_dir: String,
    total_cost: f64,
    total_tokens: i64,
    first_at: i64,
    last_at: i64,
    source_types: Vec<String>,
}

/// 合并 Codex per-request session 为真正的 Codex 会话。
/// `codex_mapping`: old_session_id → codex_jsonl_session_id
/// 非 Codex session 原样保留。
fn merge_codex_sessions(
    sessions: &[SessionBreakdown],
    codex_mapping: &HashMap<String, String>,
    project_map: &HashMap<String, String>,
    source_type_map: &HashMap<String, String>,
    session_costs: &HashMap<String, f64>,
) -> Vec<MergedSession> {
    use std::collections::hash_map::Entry;

    // group_key → (requests, tokens, cost, first_at, last_at, project_dir, source_types)
    struct Acc {
        requests: i64,
        total_tokens: i64,
        total_cost: f64,
        first_at: i64,
        last_at: i64,
        project_dir: String,
        source_types: std::collections::HashSet<String>,
    }

    let mut merged: HashMap<String, Acc> = HashMap::new();
    for s in sessions {
        let (key, is_codex) = match codex_mapping.get(&s.session_id) {
            Some(codex_sid) => (codex_sid.clone(), true),
            None => (s.session_id.clone(), false),
        };
        let tokens = s.input_tokens + s.output_tokens + s.cache_read + s.cache_creation;
        let cost = session_costs.get(&s.session_id).copied().unwrap_or(0.0);
        let project = project_map.get(&s.session_id).cloned().unwrap_or_default();

        match merged.entry(key) {
            Entry::Occupied(mut e) => {
                let acc = e.get_mut();
                acc.requests += s.requests;
                acc.total_tokens += tokens;
                acc.total_cost += cost;
                acc.first_at = acc.first_at.min(s.first_at);
                acc.last_at = acc.last_at.max(s.last_at);
                if acc.project_dir.is_empty() && !project.is_empty() {
                    acc.project_dir = project;
                }
                if let Some(st) = source_type_map.get(&s.session_id) {
                    acc.source_types.insert(st.clone());
                }
            }
            Entry::Vacant(e) => {
                let mut source_types = std::collections::HashSet::new();
                if let Some(st) = source_type_map.get(&s.session_id) {
                    source_types.insert(st.clone());
                }
                if is_codex && !source_types.contains("codex") {
                    source_types.insert("codex".to_string());
                }
                e.insert(Acc {
                    requests: s.requests,
                    total_tokens: tokens,
                    total_cost: cost,
                    first_at: s.first_at,
                    last_at: s.last_at,
                    project_dir: project,
                    source_types,
                });
            }
        }
    }

    merged.into_iter().map(|(sid, acc)| MergedSession {
        session_id: sid,
        project_dir: acc.project_dir,
        total_cost: acc.total_cost,
        total_tokens: acc.total_tokens,
        first_at: acc.first_at,
        last_at: acc.last_at,
        source_types: acc.source_types.into_iter().collect(),
    }).collect()
}

/// 构建反向映射：codex_jsonl_session_id → [原始 CC-Switch session_id 列表]
fn build_reverse_mapping(fwd: &HashMap<String, String>) -> HashMap<String, Vec<String>> {
    let mut rev: HashMap<String, Vec<String>> = HashMap::new();
    for (old, new) in fwd {
        rev.entry(new.clone()).or_default().push(old.clone());
    }
    rev
}

/// 组装二级会话详情，包含 Codex session 的合并聚合。
fn build_session_details(
    all_sessions: &[SessionBreakdown],
    requested_ids: &[String],
    codex_fwd: &HashMap<String, String>,
    codex_rev: &HashMap<String, Vec<String>>,
    session_costs: &HashMap<String, f64>,
    session_model_costs: &HashMap<String, HashMap<String, crate::services::precompute::SessionModelCostData>>,
    max_context_widths: &HashMap<String, i64>,
    timestamps_map: &HashMap<String, Vec<i64>>,
    title_map: &HashMap<String, String>,
    project_map: &HashMap<String, String>,
    source_type_map: &HashMap<String, String>,
    source_path_map: &HashMap<String, String>,
) -> Vec<ProjectSessionDetail> {
    let session_map: HashMap<&String, &SessionBreakdown> = all_sessions.iter()
        .map(|s| (&s.session_id, s)).collect();

    let codex_target_set: std::collections::HashSet<&String> = codex_fwd.values().collect();

    // 对每个请求的 session_id，找出其对应的原始 session_id 列表
    let mut result = Vec::new();
    for req_id in requested_ids {
        let original_ids = codex_rev.get(req_id)
            .map(|v| v.as_slice())
            .unwrap_or(std::slice::from_ref(req_id));
        let is_codex_merged = original_ids.len() > 1 || codex_target_set.contains(req_id);

        // 聚合所有原始 session 的数据
        let mut total_requests = 0i64;
        let mut total_input = 0i64;
        let mut total_output = 0i64;
        let mut total_cache_read = 0i64;
        let mut total_cache_creation = 0i64;
        let mut total_cost = 0.0;
        let mut first_at = i64::MAX;
        let mut last_at = 0i64;
        let mut max_ctx = 0i64;
        let mut all_timestamps: Vec<i64> = Vec::new();
        let mut model_cost_agg: HashMap<String, crate::services::precompute::SessionModelCostData> = HashMap::new();
        let mut best_title: Option<String> = None;
        let mut best_project: Option<String> = None;
        let mut best_source_type: Option<String> = None;

        for orig_id in original_ids {
            if let Some(s) = session_map.get(orig_id) {
                total_requests += s.requests;
                total_input += s.input_tokens;
                total_output += s.output_tokens;
                total_cache_read += s.cache_read;
                total_cache_creation += s.cache_creation;
                first_at = first_at.min(s.first_at);
                last_at = last_at.max(s.last_at);
            }
            total_cost += session_costs.get(orig_id).copied().unwrap_or(0.0);
            max_ctx = max_ctx.max(max_context_widths.get(orig_id).copied().unwrap_or(0));
            if let Some(ts) = timestamps_map.get(orig_id) { all_timestamps.extend(ts); }

            // 聚合模型费用
            if let Some(mc) = session_model_costs.get(orig_id) {
                for (model, data) in mc {
                    let acc = model_cost_agg.entry(model.clone()).or_insert_with(|| crate::services::precompute::SessionModelCostData {
                        cost: 0.0, input_tokens: 0, output_tokens: 0, cache_read: 0, cache_creation: 0,
                        breakdown: vec![0.0; 4], tier_costs: HashMap::new(), tier_tokens: HashMap::new(),
                    });
                    acc.cost += data.cost;
                    acc.input_tokens += data.input_tokens;
                    acc.output_tokens += data.output_tokens;
                    acc.cache_read += data.cache_read;
                    acc.cache_creation += data.cache_creation;
                    for (i, b) in data.breakdown.iter().enumerate() { if i < 4 { acc.breakdown[i] += b; } }
                    for (t, c) in &data.tier_costs { *acc.tier_costs.entry(*t).or_insert(0.0) += c; }
                    for (t, n) in &data.tier_tokens { *acc.tier_tokens.entry(*t).or_insert(0) += n; }
                }
            }

            if best_title.is_none() { best_title = title_map.get(orig_id).cloned(); }
            if best_project.is_none() { best_project = project_map.get(orig_id).cloned(); }
            if best_source_type.is_none() { best_source_type = source_type_map.get(orig_id).cloned(); }
        }

        if is_codex_merged {
            best_source_type = Some("codex".to_string());
        }

        let cache_hit_rate = if (total_input + total_cache_read + total_cache_creation) > 0 {
            total_cache_read as f64 / (total_input + total_cache_read + total_cache_creation) as f64
        } else { 0.0 };

        let model_breakdown: Vec<SessionModelCostEntry> = model_cost_agg.iter().map(|(model, data)| {
            let mut tier_vec: Vec<ContextTierCost> = data.tier_costs.iter()
                .filter(|(_, c)| **c > 0.0)
                .map(|(threshold, cost)| ContextTierCost {
                    threshold: *threshold,
                    cost: *cost,
                    tokens: data.tier_tokens.get(threshold).copied().unwrap_or(0),
                })
                .collect();
            tier_vec.sort_by_key(|t| t.threshold);
            SessionModelCostEntry {
                session_id: req_id.clone(),
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
        }).collect();

        if first_at == i64::MAX { first_at = 0; }
        if total_requests == 0 { continue; }

        result.push(ProjectSessionDetail {
            session_id: req_id.clone(),
            request_count: total_requests,
            total_tokens: total_input + total_output + total_cache_read + total_cache_creation,
            total_cost,
            start_time: first_at,
            end_time: last_at,
            duration_sec: if last_at > first_at { last_at - first_at } else { 0 },
            max_context_width: max_ctx,
            cache_hit_rate,
            timestamps: all_timestamps,
            model_breakdown,
            title: best_title,
            project_dir: best_project,
            source_path: original_ids.iter()
                .find_map(|id| source_path_map.get(id).cloned())
                .or_else(|| source_path_map.get(req_id).cloned()),
            source_type: best_source_type,
        });
    }
    result
}
