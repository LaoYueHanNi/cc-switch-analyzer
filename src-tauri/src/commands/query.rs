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
        handles.into_iter().filter_map(|h| h.join().unwrap()).collect()
    })
}


#[tauri::command]
pub fn query_summary(params: FilterParams, state: State<AppState>) -> Result<SummaryData, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_summary(&records))
}

#[tauri::command]
pub fn query_by_model(params: FilterParams, state: State<AppState>) -> Result<Vec<ModelBreakdown>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_model_breakdown(&records))
}

#[tauri::command]
pub fn query_by_provider(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderBreakdown>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    let provider_names = collect_provider_names(&sources);
    Ok(aggregate_provider_breakdown(&records, &provider_names))
}

#[tauri::command]
pub fn query_provider_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<ProviderModelToken>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_provider_model_tokens(&records))
}

#[tauri::command]
pub fn query_daily_trend(params: FilterParams, state: State<AppState>) -> Result<Vec<DailyTrendRow>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    let tz_offset = params.tz_offset.unwrap_or(0);
    Ok(aggregate_daily_trend(&records, tz_offset))
}

#[tauri::command]
pub fn query_hourly_trend(params: FilterParams, state: State<AppState>) -> Result<Vec<DailyTrendRow>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    let tz_offset = params.tz_offset.unwrap_or(0);
    Ok(aggregate_hourly_trend(&records, tz_offset))
}

#[tauri::command]
pub fn query_sessions(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionBreakdown>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let records = fetch_deduped_records(&sources, &params)?;
    Ok(aggregate_session_breakdown(&records))
}

#[tauri::command]
pub fn query_session_model_tokens(params: FilterParams, state: State<AppState>) -> Result<Vec<SessionModelToken>, String> {
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
                    true
                })
                .cloned()
                .collect();
            return Ok(filtered);
        }
    }
    // 缓存为空，回退到全量并行查询 + 去重
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let all: Vec<Vec<SessionRequestToken>> = parallel_query(&sources, params.clone(), "session_request_tokens", |e, p| e.source.get_session_request_tokens(p));
    Ok(dedup_request_tokens(all.into_iter().flatten().collect()))
}

#[tauri::command]
pub fn query_session_timestamps(session_ids: Vec<String>, state: State<AppState>) -> Result<HashMap<String, Vec<i64>>, String> {
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
            handles.into_iter().filter_map(|h| h.join().unwrap()).collect()
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
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let results: Vec<Vec<RealtimeBucket>> = parallel_query(&sources, (), "realtime", |e, _| e.source.get_minute_level_token_trend());
    Ok(merge_realtime_buckets(results))
}

#[tauri::command]
pub fn query_realtime_logs(since: Option<i64>, state: State<AppState>) -> Result<Vec<RealtimeRequestLog>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);
    let all_raw = run_streaming_dedup(&sources, since);
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
    let tier_buckets = aggregate_model_context_tier_buckets(&records, tz_offset, &thresholds);

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

    let (tier_costs, ctx_model_costs, ctx_model_breakdown) = build_context_tier_and_model_costs(&tier_buckets, pricing);
    precomputed.model_context_tier_costs = tier_costs;

    let mut compare_buckets_map: HashMap<String, Vec<CompareBucket>> = HashMap::new();
    for bucket in &tier_buckets {
        let cb = CompareBucket {
            threshold: bucket.context_tier.max(0),
            representative_epoch: bucket.representative_epoch,
            input_tokens: bucket.input_tokens,
            output_tokens: bucket.output_tokens,
            cache_read: bucket.cache_read,
            cache_creation: bucket.cache_creation,
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
            handles.into_iter().map(|h| h.join().unwrap()).collect()
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
            handles.into_iter().map(|h| h.join().unwrap()).collect()
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

    let session_costs = compute_session_costs(&session_request_tokens, &pricing);
    let session_model_costs = compute_session_model_costs(&session_request_tokens, &session_model_tokens, &pricing);

    let enriched: Vec<SessionWithCost> = filtered_sessions
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
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);

    // 1. 带过滤的 session_breakdown
    let all_sessions: Vec<Vec<SessionBreakdown>> = parallel_query(&sources, params.clone(), "session_breakdown", |e, p| e.source.get_session_breakdown(p));
    let all_sessions: Vec<SessionBreakdown> = all_sessions.into_iter().flatten().collect();

    if all_sessions.is_empty() { return Ok(vec![]); }

    let all_ids: Vec<String> = all_sessions.iter().map(|s| s.session_id.clone()).collect();

    // 2. 解析项目目录
    let (project_map, _) = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        resolve_session_projects(&all_ids, &app_db, &sources)?
    };

    // 3. 计算基础费用（不做模型分解）
    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    let all_request_tokens: Vec<SessionRequestToken> = {
        let mut tokens = Vec::new();
        for entry in sources.iter() {
            if let Ok(t) = entry.source.get_session_request_tokens_for_ids(&params, &all_ids) {
                tokens.extend(t);
            }
        }
        tokens
    };
    let session_costs = compute_session_costs(&all_request_tokens, &pricing);

    // 4. 按 projectDir 分组聚合（保留 session_ids）
    let mut groups: HashMap<String, (i64, f64, i64, i64, i64, Vec<String>)> = HashMap::new();
    for s in &all_sessions {
        let dir = project_map.get(&s.session_id).map(|s| s.as_str()).unwrap_or("");
        let cost = session_costs.get(&s.session_id).copied().unwrap_or(0.0);
        let tokens = s.input_tokens + s.output_tokens + s.cache_read + s.cache_creation;
        let entry = groups.entry(dir.to_string()).or_default();
        entry.0 += 1;
        entry.1 += cost;
        entry.2 += tokens;
        entry.3 = entry.3.min(s.first_at);
        entry.4 = entry.4.max(s.last_at);
        entry.5.push(s.session_id.clone());
    }

    // 5. 组装结果
    let mut result: Vec<ProjectGroupStats> = groups.into_iter().map(|(dir, (count, cost, tokens, first, last, session_ids))| {
        let display_name = if dir.is_empty() {
            "未知项目".to_string()
        } else {
            std::path::Path::new(&dir)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&dir)
                .to_string()
        };
        ProjectGroupStats {
            project_dir: dir,
            display_name,
            session_count: count,
            total_cost: cost,
            total_tokens: tokens,
            first_at: first,
            last_at: last,
            session_ids,
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

    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    require_sources!(sources);

    // 1. 从 sessions 表拿标题和目录（不查 JSONL）
    let (title_map, project_map) = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let cached = app_db.get_sessions(&session_ids)?;
        let mut titles = HashMap::new();
        let mut projects = HashMap::new();
        for (sid, (dir, title, _)) in &cached {
            if !title.is_empty() { titles.insert(sid.clone(), title.clone()); }
            if !dir.is_empty() { projects.insert(sid.clone(), dir.clone()); }
        }
        (titles, projects)
    };

    // 2. 只查这些 session 的 breakdown（用于基础统计）
    let all_sessions: Vec<SessionBreakdown> = std::thread::scope(|s| {
        let handles: Vec<_> = sources.iter().map(|entry| {
            let p = params.clone();
            let label = entry.db_type.label().to_string();
            s.spawn(move || {
                entry.source.get_session_breakdown(&p).map_err(|e| {
                    log::warn!("[QUERY] session_breakdown 数据源({}) 查询失败: {}", label, e); e
                }).ok().unwrap_or_default()
            })
        }).collect();
        let all: Vec<SessionBreakdown> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        let id_set: std::collections::HashSet<String> = session_ids.iter().cloned().collect();
        all.into_iter().filter(|s| id_set.contains(&s.session_id)).collect()
    });

    if all_sessions.is_empty() { return Ok(vec![]); }

    let filtered_ids: Vec<String> = all_sessions.iter().map(|s| s.session_id.clone()).collect();

    // 3. 加载完整数据
    let (all_request_tokens, all_model_tokens, max_context_widths, timestamps_map) = {
        let detail_data: Vec<_> = std::thread::scope(|s| {
            let handles: Vec<_> = sources.iter().map(|entry| {
                let p = params.clone();
                let ids = filtered_ids.clone();
                s.spawn(move || {
                    let req = entry.source.get_session_request_tokens_for_ids(&p, &ids).ok();
                    let mdl = entry.source.get_session_model_tokens_for_ids(&p, &ids).ok();
                    (req, mdl)
                })
            }).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        let all_req: Vec<SessionRequestToken> = detail_data.iter()
            .filter_map(|d| d.0.clone()).flatten().collect();
        let all_mdl: Vec<SessionModelToken> = detail_data.iter()
            .filter_map(|d| d.1.clone()).flatten().collect();

        let mut max_ctx: HashMap<String, i64> = HashMap::new();
        let mut ts_map: HashMap<String, Vec<i64>> = HashMap::new();
        for entry in sources.iter() {
            if let Ok(m) = entry.source.get_session_max_context_widths(&filtered_ids) {
                for (k, v) in m { max_ctx.insert(k, v); }
            }
            if let Ok(t) = entry.source.get_session_timestamps(&filtered_ids) {
                for (k, v) in t { ts_map.entry(k).or_default().extend(v); }
            }
        }
        (all_req, all_mdl, max_ctx, ts_map)
    };

    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    let session_costs = compute_session_costs(&all_request_tokens, &pricing);
    let session_model_costs = compute_session_model_costs(&all_request_tokens, &all_model_tokens, &pricing);

    // 5. 批量构建 source_path（一次遍历目录匹配所有 ClaudeCode session）
    let source_path_map: HashMap<String, String> = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let all_cached = app_db.get_sessions(&filtered_ids)?;
        let claude_ids: Vec<String> = filtered_ids.iter()
            .filter(|sid| all_cached.get(sid.as_str()).map(|(_, _, s)| s.as_str()) == Some("claudecode"))
            .cloned()
            .collect();
        find_jsonl_batch(&claude_ids)
    };

    // 6. 组装结果
    let details: Vec<ProjectSessionDetail> = all_sessions.into_iter().map(|s| {
        let cost = session_costs.get(&s.session_id).copied().unwrap_or(0.0);
        let cache_hit_rate = if (s.input_tokens + s.cache_read) > 0 {
            s.cache_read as f64 / (s.input_tokens + s.cache_read) as f64
        } else { 0.0 };

        let model_breakdown: Vec<SessionModelCostEntry> = session_model_costs
            .get(&s.session_id)
            .map(|mc| {
                mc.iter().map(|(model, data)| {
                    let mut tier_vec: Vec<ContextTierCost> = data
                        .tier_costs.iter()
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
                }).collect()
            })
            .unwrap_or_default();

        ProjectSessionDetail {
            session_id: s.session_id.clone(),
            request_count: s.requests,
            total_tokens: s.input_tokens + s.output_tokens + s.cache_read + s.cache_creation,
            total_cost: cost,
            start_time: s.first_at,
            end_time: s.last_at,
            duration_sec: s.last_at - s.first_at,
            max_context_width: max_context_widths.get(&s.session_id).copied().unwrap_or(0),
            cache_hit_rate,
            timestamps: timestamps_map.get(&s.session_id).cloned().unwrap_or_default(),
            model_breakdown,
            title: title_map.get(&s.session_id).cloned(),
            project_dir: project_map.get(&s.session_id).cloned(),
            source_path: source_path_map.get(&s.session_id).cloned(),
        }
    }).collect();

    Ok(details)
}
