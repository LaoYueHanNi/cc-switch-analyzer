use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::thread;

use chrono::TimeZone;

use super::data_source::SourceEntry;
use super::dedup::RequestFingerprint;
use crate::models::*;
use crate::utils::SESSION_TOP_N;

/// 从所有数据源收集 provider_id → provider_name 映射。
pub fn collect_provider_names(sources: &[SourceEntry]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for entry in sources {
        if let Ok(providers) = entry.source.get_providers() {
            for p in providers {
                map.entry(p.id).or_insert(p.name);
            }
        }
    }
    map
}

/// 构建 provider_id → 数据源 canonical 名映射（动态供应商归并到所属数据源，
/// 如 CCS 的 UUID、OpenCode 的 providerID）。
pub fn build_provider_to_db_map(sources: &[SourceEntry]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for s in sources.iter().filter(|s| s.enabled) {
        if let Ok(providers) = s.source.get_providers() {
            for p in providers {
                map.insert(p.id, s.db_type.label().to_string());
            }
        }
    }
    map
}

/// 数据源级过滤解析：当筛选的 provider_id 恰好等于某数据源 canonical 名
/// （如 "CCS"/"OpenCode"）时，只保留该数据源并剥离内部 provider 过滤
/// （该源返回全量记录）；空值或未命中（历史内部 provider id，如 CCS UUID、
/// OpenCode providerID）时保持原 provider 过滤语义。
pub fn scope_to_provider_source<'a>(
    sources: &'a [SourceEntry],
    params: &FilterParams,
) -> (Vec<&'a SourceEntry>, FilterParams) {
    let Some(pid) = params.provider_id.as_deref().filter(|p| !p.is_empty()) else {
        return (sources.iter().collect(), params.clone());
    };
    if let Some(matched) = sources.iter().find(|s| s.enabled && s.db_type.label() == pid) {
        let mut scoped = params.clone();
        scoped.provider_id = None;
        (vec![matched], scoped)
    } else {
        (sources.iter().collect(), params.clone())
    }
}

/// 流式去重 Pipeline。
///
/// 并行查询所有数据源，通过 channel 流式传输记录，
/// 消费端边收边去重，真正流式处理（不在内存中积累全量数据再过滤）。
///
/// 数据流：Producer(源1) ──┐
///         Producer(源2) ──┼→ channel → Deduplicator(逐条) → Vec<Record>
///         Producer(源N) ──┘
pub fn run_streaming_dedup(
    sources: &[SourceEntry],
    since: Option<i64>,
) -> Vec<(String, String, String, i64, i64, i64, i64, i64, i64, bool)> {
    let (tx, rx) = mpsc::channel();

    // 并行 Producer：每个数据源一个线程，逐条发送到 channel
    thread::scope(|s| {
        for entry in sources.iter().filter(|s| s.enabled) {
            let tx = tx.clone();
            s.spawn(move || {
                if let Err(e) = entry.source.stream_records(since, &mut |record| {
                    let _ = tx.send(record);
                }) {
                    log::warn!("[PIPELINE] 数据源({}) stream_records 失败: {}", entry.db_type.label(), e);
                }
            });
        }
        drop(tx);
    });

    // Consumer：边收边去重（不在内存中积累全量再过滤）
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for record in rx {
        let fp = RequestFingerprint::new(&record.0, &record.1, record.4, record.5, record.7);
        if seen.insert(fp) {
            result.push(record);
        }
    }
    result
}

// ========== 中心去重管道 ==========

/// 并行从所有数据源获取过滤后的原始记录，然后去重。
///
/// 这是新查询管道的入口：所有聚合查询应从这里取去重后的数据。
pub fn fetch_deduped_records(
    sources: &[SourceEntry],
    params: &FilterParams,
) -> Result<Vec<RawRecord>, String> {
    // 数据源级过滤：provider_id 命中数据源 canonical 名时，只查询该源并剥离内部 provider 过滤
    let (scoped, scoped_params) = scope_to_provider_source(sources, params);
    let active: Vec<&SourceEntry> = scoped.into_iter().filter(|s| s.enabled).collect();
    if active.is_empty() {
        return Ok(Vec::new());
    }
    let all_records: Vec<Vec<RawRecord>> = std::thread::scope(|s| {
        let handles: Vec<_> = active.into_iter().map(|entry| {
            let p = scoped_params.clone();
            let label = entry.db_type.label().to_string();
            s.spawn(move || {
                entry.source.get_filtered_records(&p).map_err(|e| {
                    log::warn!("[PIPELINE] 数据源({}) get_filtered_records 失败: {}", label, e);
                    e
                }).ok()
            })
        }).collect();
        handles.into_iter().filter_map(|h| match h.join() {
            Ok(v) => v,
            Err(_) => {
                log::error!("[PIPELINE] 并行查询线程 panic");
                None
            }
        }).collect()
    });

    let mut flat: Vec<RawRecord> = all_records.into_iter().flatten().collect();

    // AIProxy 优先：去重先到先保留，AIProxy 有原始 target_model 和准确 input
    flat.sort_by(|a, b| {
        let a_priority = a.provider_id == "AIProxy";
        let b_priority = b.provider_id == "AIProxy";
        b_priority.cmp(&a_priority)
    });
    Ok(super::dedup::dedup_records(flat))
}

// ========== 时间辅助函数 ==========

/// 将 epoch 秒转为 YYYY-MM-DD 格式的日期字符串。
/// `tz_offset` 为 UTC 偏移小时数（如东八区为 8）。
pub fn to_day(epoch: i64, tz_offset: i64) -> String {
    let offset_secs = tz_offset * 3600;
    chrono::Utc.timestamp_opt(epoch + offset_secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// 将 epoch 秒转为 HH:00 格式的小时字符串。
/// `tz_offset` 为 UTC 偏移小时数（如东八区为 8）。
pub fn to_hour(epoch: i64, tz_offset: i64) -> String {
    let offset_secs = tz_offset * 3600;
    chrono::Utc.timestamp_opt(epoch + offset_secs, 0)
        .single()
        .map(|dt| dt.format("%H:00").to_string())
        .unwrap_or_default()
}

// ========== 内存聚合函数 ==========

/// 聚合总览数据：COUNT, SUM(input/output/cache_read/cache_creation), AVG(latency)
pub fn aggregate_summary(records: &[RawRecord]) -> SummaryData {
    if records.is_empty() {
        return SummaryData {
            total_requests: 0,
            success_count: 0,
            total_input: 0,
            total_output: 0,
            total_cache_read: 0,
            total_cache_creation: 0,
            avg_latency: 0.0,
        };
    }

    let total_requests = records.len() as i64;
    let total_input: i64 = records.iter().map(|r| r.input_tokens).sum();
    let total_output: i64 = records.iter().map(|r| r.output_tokens).sum();
    let total_cache_read: i64 = records.iter().map(|r| r.cache_read).sum();
    let total_cache_creation: i64 = records.iter().map(|r| r.cache_creation).sum();
    let latency_sum: f64 = records.iter().map(|r| r.latency as f64).sum();

    SummaryData {
        total_requests,
        success_count: total_requests, // 所有记录视为成功
        total_input,
        total_output,
        total_cache_read,
        total_cache_creation,
        avg_latency: if total_requests > 0 { latency_sum / total_requests as f64 } else { 0.0 },
    }
}

/// 聚合模型分组：GROUP BY model, COUNT, SUMs
pub fn aggregate_model_breakdown(records: &[RawRecord]) -> Vec<ModelBreakdown> {
    let mut map: HashMap<String, (i64, i64, i64, i64, i64)> = HashMap::new();
    for r in records {
        let e = map.entry(r.model.clone()).or_insert((0, 0, 0, 0, 0));
        e.0 += 1;
        e.1 += r.input_tokens;
        e.2 += r.output_tokens;
        e.3 += r.cache_read;
        e.4 += r.cache_creation;
    }
    let mut v: Vec<ModelBreakdown> = map.into_iter()
        .map(|(model, (requests, input_tokens, output_tokens, cache_read, cache_creation))| {
            ModelBreakdown { model, requests, input_tokens, output_tokens, cache_read, cache_creation }
        })
        .collect();
    v.sort_by(|a, b| b.requests.cmp(&a.requests));
    v
}

/// 聚合供应商分组（一级 = 数据源粒度）：
/// GROUP BY db_type, COUNT, AVG(latency), success_rate=100.0
/// CCS/OpenCode 等动态 provider 在此归并到所属数据源（二级明细仍保留在原始记录中）
pub fn aggregate_provider_breakdown(records: &[RawRecord], _provider_names: &HashMap<String, String>) -> Vec<ProviderBreakdown> {
    struct Acc { requests: i64, latency_sum: f64 }
    let mut map: HashMap<String, Acc> = HashMap::new();
    for r in records {
        let key = if r.db_type.is_empty() { r.provider_id.clone() } else { r.db_type.clone() };
        let e = map.entry(key).or_insert(Acc { requests: 0, latency_sum: 0.0 });
        e.requests += 1;
        e.latency_sum += r.latency as f64;
    }
    let mut v: Vec<ProviderBreakdown> = map.into_iter()
        .map(|(provider_id, acc)| {
            ProviderBreakdown {
                provider_name: provider_id.clone(),
                provider_id,
                requests: acc.requests,
                successes: acc.requests,
                success_rate: 100.0,
                avg_latency: if acc.requests > 0 { acc.latency_sum / acc.requests as f64 } else { 0.0 },
            }
        })
        .collect();
    v.sort_by(|a, b| b.requests.cmp(&a.requests));
    v
}

/// 将 provider_costs（二级 provider_id 粒度）归并到一级数据源粒度（db_type），
/// 与 aggregate_provider_breakdown 的 key 规则保持一致：db_type 非空时用 db_type，
/// 否则保留原 provider_id。修复 ByProvider 卡片按数据源名查 providerCosts 命中失败的问题
/// （OpenCode 等动态 provider 的 provider_id 与 db_type 取值不一致时，查不到会回退 0）。
pub fn merge_provider_costs_to_db_type(
    records: &[RawRecord],
    provider_costs: &mut HashMap<String, f64>,
) {
    let mut provider_to_db: HashMap<String, String> = HashMap::new();
    for r in records {
        if !r.db_type.is_empty() {
            provider_to_db
                .entry(r.provider_id.clone())
                .or_insert_with(|| r.db_type.clone());
        }
    }
    if provider_to_db.is_empty() {
        return;
    }
    let mut merged: HashMap<String, f64> = HashMap::new();
    for (pid, cost) in std::mem::take(provider_costs) {
        let key = provider_to_db.get(&pid).cloned().unwrap_or(pid);
        *merged.entry(key).or_insert(0.0) += cost;
    }
    *provider_costs = merged;
}

/// 聚合组合分组：GROUP BY (day, provider_id, model), COUNT, SUMs, SUM(latency)
/// 注意：此函数名与 precompute::aggregate_combined_breakdown 不同（后者接收 CombinedBreakdownRow）。
pub fn aggregate_combined_records(records: &[RawRecord], tz_offset: i64) -> Vec<CombinedBreakdownRow> {
    let mut map: HashMap<(String, String, String), (i64, i64, i64, i64, i64, f64)> = HashMap::new();
    for r in records {
        let day = to_day(r.created_at, tz_offset);
        let key = (day, r.provider_id.clone(), r.model.clone());
        let e = map.entry(key).or_insert((0, 0, 0, 0, 0, 0.0));
        e.0 += 1;
        e.1 += r.input_tokens;
        e.2 += r.output_tokens;
        e.3 += r.cache_read;
        e.4 += r.cache_creation;
        e.5 += r.latency as f64;
    }
    let mut v: Vec<CombinedBreakdownRow> = map.into_iter()
        .map(|((day, provider_id, model), (requests, input_tokens, output_tokens, cache_read, cache_creation, latency_sum))| {
            CombinedBreakdownRow { day, provider_id, model, requests, input_tokens, output_tokens, cache_read, cache_creation, latency_sum }
        })
        .collect();
    v.sort_by(|a, b| (&a.day, &a.provider_id, &a.model).cmp(&(&b.day, &b.provider_id, &b.model)));
    v
}

/// 聚合供应商-模型 Token 分组：GROUP BY (provider_id, model), SUMs
pub fn aggregate_provider_model_tokens(records: &[RawRecord]) -> Vec<ProviderModelToken> {
    let mut map: HashMap<(String, String), (i64, i64, i64, i64)> = HashMap::new();
    for r in records {
        let key = (r.provider_id.clone(), r.model.clone());
        let e = map.entry(key).or_insert((0, 0, 0, 0));
        e.0 += r.input_tokens;
        e.1 += r.output_tokens;
        e.2 += r.cache_read;
        e.3 += r.cache_creation;
    }
    map.into_iter()
        .map(|((provider_id, model), (input_tokens, output_tokens, cache_read, cache_creation))| {
            ProviderModelToken { provider_id, model, input_tokens, output_tokens, cache_read, cache_creation }
        })
        .collect()
}

/// 聚合每日趋势：GROUP BY (day, model), COUNT, SUMs, AVG(latency)
pub fn aggregate_daily_trend(records: &[RawRecord], tz_offset: i64) -> Vec<DailyTrendRow> {
    let mut map: HashMap<(String, String), (i64, i64, i64, i64, i64, f64)> = HashMap::new();
    for r in records {
        let day = to_day(r.created_at, tz_offset);
        let key = (day, r.model.clone());
        let e = map.entry(key).or_insert((0, 0, 0, 0, 0, 0.0));
        e.0 += 1;
        e.1 += r.input_tokens;
        e.2 += r.output_tokens;
        e.3 += r.cache_read;
        e.4 += r.cache_creation;
        e.5 += r.latency as f64;
    }
    let mut v: Vec<DailyTrendRow> = map.into_iter()
        .map(|((day, model), (requests, input_tokens, output_tokens, cache_read, cache_creation, latency_sum))| {
            DailyTrendRow {
                day, model, requests, input_tokens, output_tokens, cache_read, cache_creation,
                avg_latency: if requests > 0 { latency_sum / requests as f64 } else { 0.0 },
            }
        })
        .collect();
    v.sort_by(|a, b| (&a.day, &a.model).cmp(&(&b.day, &b.model)));
    v
}

/// 聚合小时趋势：GROUP BY (hour, model), COUNT, SUMs, AVG(latency)
pub fn aggregate_hourly_trend(records: &[RawRecord], tz_offset: i64) -> Vec<DailyTrendRow> {
    let mut map: HashMap<(String, String), (i64, i64, i64, i64, i64, f64)> = HashMap::new();
    for r in records {
        let hour = to_hour(r.created_at, tz_offset);
        let key = (hour, r.model.clone());
        let e = map.entry(key).or_insert((0, 0, 0, 0, 0, 0.0));
        e.0 += 1;
        e.1 += r.input_tokens;
        e.2 += r.output_tokens;
        e.3 += r.cache_read;
        e.4 += r.cache_creation;
        e.5 += r.latency as f64;
    }
    let mut v: Vec<DailyTrendRow> = map.into_iter()
        .map(|((hour_label, model), (requests, input_tokens, output_tokens, cache_read, cache_creation, latency_sum))| {
            DailyTrendRow {
                day: hour_label, model, requests, input_tokens, output_tokens, cache_read, cache_creation,
                avg_latency: if requests > 0 { latency_sum / requests as f64 } else { 0.0 },
            }
        })
        .collect();
    v.sort_by(|a, b| (&a.day, &a.model).cmp(&(&b.day, &b.model)));
    v
}

/// 聚合会话分组：GROUP BY session_id (非空), COUNT, SUMs, MIN/MAX(created_at), LIMIT SESSION_TOP_N
/// max_context_width 暂设为 0（由后续查询补充）
pub fn aggregate_session_breakdown(records: &[RawRecord]) -> Vec<SessionBreakdown> {
    struct Acc {
        requests: i64,
        input_tokens: i64,
        output_tokens: i64,
        cache_read: i64,
        cache_creation: i64,
        first_at: i64,
        last_at: i64,
    }

    let mut map: HashMap<String, Acc> = HashMap::new();
    for r in records {
        if r.session_id.is_empty() {
            continue;
        }
        let e = map.entry(r.session_id.clone()).or_insert_with(|| Acc {
            requests: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read: 0,
            cache_creation: 0,
            first_at: r.created_at,
            last_at: r.created_at,
        });
        e.requests += 1;
        e.input_tokens += r.input_tokens;
        e.output_tokens += r.output_tokens;
        e.cache_read += r.cache_read;
        e.cache_creation += r.cache_creation;
        if r.created_at < e.first_at { e.first_at = r.created_at; }
        if r.created_at > e.last_at { e.last_at = r.created_at; }
    }

    let mut v: Vec<SessionBreakdown> = map.into_iter()
        .map(|(session_id, acc)| SessionBreakdown {
            session_id,
            requests: acc.requests,
            max_context_width: 0,
            input_tokens: acc.input_tokens,
            output_tokens: acc.output_tokens,
            cache_read: acc.cache_read,
            cache_creation: acc.cache_creation,
            first_at: acc.first_at,
            last_at: acc.last_at,
        })
        .collect();
    v.sort_by(|a, b| b.requests.cmp(&a.requests));
    v.truncate(SESSION_TOP_N as usize);
    v
}

/// 聚合会话-模型 Token 分组：先取 top SESSION_TOP_N sessions，再 GROUP BY (session_id, model)
pub fn aggregate_session_model_tokens(records: &[RawRecord]) -> Vec<SessionModelToken> {
    // 先聚合每个 session 的请求数，取 top SESSION_TOP_N
    let mut session_requests: HashMap<String, i64> = HashMap::new();
    for r in records {
        if r.session_id.is_empty() {
            continue;
        }
        *session_requests.entry(r.session_id.clone()).or_insert(0) += 1;
    }
    let mut session_list: Vec<(String, i64)> = session_requests.into_iter().collect();
    session_list.sort_by(|a, b| b.1.cmp(&a.1));
    let top_session_ids: HashSet<String> = session_list.into_iter()
        .take(SESSION_TOP_N as usize)
        .map(|(id, _)| id)
        .collect();

    // 在 top sessions 中做 session_id + model 聚合
    let mut map: HashMap<(String, String), (i64, i64, i64, i64)> = HashMap::new();
    for r in records {
        if r.session_id.is_empty() || !top_session_ids.contains(&r.session_id) {
            continue;
        }
        let key = (r.session_id.clone(), r.model.clone());
        let e = map.entry(key).or_insert((0, 0, 0, 0));
        e.0 += r.input_tokens;
        e.1 += r.output_tokens;
        e.2 += r.cache_read;
        e.3 += r.cache_creation;
    }

    map.into_iter()
        .map(|((session_id, model), (input_tokens, output_tokens, cache_read, cache_creation))| {
            SessionModelToken { session_id, model, input_tokens, output_tokens, cache_read, cache_creation }
        })
        .collect()
}

/// 聚合上下文档位桶：
/// context_width = input_tokens + cache_read
/// CASE WHEN context_width >= threshold THEN threshold
/// GROUP BY (model, day, tier, slot_key), MIN(created_at) as representative_epoch
pub fn aggregate_model_context_tier_buckets(
    records: &[RawRecord],
    tz_offset: i64,
    thresholds: &[i64],
    pricing: Option<&crate::services::pricing_engine::PricingEngine>,
) -> Vec<ModelContextTierBucket> {
    let split_slots = pricing.map(|p| p.has_any_daily_slots()).unwrap_or(false);
    if thresholds.is_empty() && !split_slots {
        return Vec::new();
    }

    // 按升序排序阈值
    let mut sorted_thresholds: Vec<i64> = thresholds.to_vec();
    sorted_thresholds.sort();

    struct Acc {
        input_tokens: i64,
        output_tokens: i64,
        cache_read: i64,
        cache_creation: i64,
        representative_epoch: i64,
    }

    let mut map: HashMap<(String, String, i64, i64), Acc> = HashMap::new();

    for r in records {
        let context_width = r.input_tokens + r.cache_read;

        // 找到匹配的档位：最大的 <= context_width 的阈值
        let tier = if sorted_thresholds.is_empty() {
            0
        } else {
            sorted_thresholds.iter()
                .rev()
                .find(|&&t| context_width >= t)
                .copied()
                .unwrap_or(0)
        };

        let slot_key = if split_slots {
            pricing
                .map(|p| p.get_matched_slot_key(&r.model, r.created_at, context_width, tz_offset))
                .unwrap_or(-1)
        } else {
            -1
        };

        let day = to_day(r.created_at, tz_offset);
        let key = (r.model.clone(), day, tier, slot_key);

        map.entry(key)
            .and_modify(|e| {
                e.input_tokens += r.input_tokens;
                e.output_tokens += r.output_tokens;
                e.cache_read += r.cache_read;
                e.cache_creation += r.cache_creation;
                // representative_epoch 取最小值
                if r.created_at < e.representative_epoch {
                    e.representative_epoch = r.created_at;
                }
            })
            .or_insert(Acc {
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
                cache_read: r.cache_read,
                cache_creation: r.cache_creation,
                representative_epoch: r.created_at,
            });
    }

    map.into_iter()
        .map(|((model, day, context_tier, slot_key), acc)| ModelContextTierBucket {
            model,
            day,
            context_tier,
            input_tokens: acc.input_tokens,
            output_tokens: acc.output_tokens,
            cache_read: acc.cache_read,
            cache_creation: acc.cache_creation,
            representative_epoch: acc.representative_epoch,
            slot_key,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY1: i64 = 1704067200; // 2024-01-01 00:00:00 UTC

    fn rec(session_id: &str, model: &str, provider_id: &str, created_at: i64, input: i64, output: i64, cache_read: i64, cache_creation: i64, latency: i64) -> RawRecord {
        RawRecord {
            session_id: session_id.to_string(),
            model: model.to_string(),
            provider_id: provider_id.to_string(),
            db_type: String::new(),
            created_at,
            input_tokens: input,
            output_tokens: output,
            cache_read,
            cache_creation,
            latency,
            is_codex: false,
        }
    }

    // ===== to_day / to_hour 时区转换 =====

    #[test]
    fn to_day_utc() {
        assert_eq!(to_day(0, 0), "1970-01-01");
        assert_eq!(to_day(DAY1, 0), "2024-01-01");
    }

    #[test]
    fn to_day_with_timezone_offset() {
        // 2023-12-31 23:00 UTC，东八区为 2024-01-01 07:00 → 跨日
        assert_eq!(to_day(DAY1 - 3600, 8), "2024-01-01");
        // 2024-01-01 02:00 UTC，西三区为 2023-12-31 23:00 → 跨日
        assert_eq!(to_day(DAY1 + 2 * 3600, -3), "2023-12-31");
    }

    #[test]
    fn to_hour_with_timezone_offset() {
        assert_eq!(to_hour(0, 0), "00:00");
        assert_eq!(to_hour(0, 8), "08:00");
        assert_eq!(to_hour(3600, 0), "01:00");
    }

    // ===== aggregate_summary =====

    #[test]
    fn aggregate_summary_empty() {
        let s = aggregate_summary(&[]);
        assert_eq!(s.total_requests, 0);
        assert_eq!(s.avg_latency, 0.0);
    }

    #[test]
    fn aggregate_summary_sums_and_avg() {
        let records = vec![
            rec("s", "m", "p", 0, 100, 200, 50, 30, 100),
            rec("s", "m", "p", 0, 300, 400, 60, 40, 200),
        ];
        let s = aggregate_summary(&records);
        assert_eq!(s.total_requests, 2);
        assert_eq!(s.success_count, 2);
        assert_eq!(s.total_input, 400);
        assert_eq!(s.total_output, 600);
        assert_eq!(s.total_cache_read, 110);
        assert_eq!(s.total_cache_creation, 70);
        assert!((s.avg_latency - 150.0).abs() < 1e-9);
    }

    // ===== aggregate_model_breakdown =====

    #[test]
    fn aggregate_model_breakdown_groups_and_sorts_desc() {
        let records = vec![
            rec("s", "A", "p", 0, 10, 0, 0, 0, 0),
            rec("s", "A", "p", 0, 20, 0, 0, 0, 0),
            rec("s", "B", "p", 0, 5, 0, 0, 0, 0),
        ];
        let v = aggregate_model_breakdown(&records);
        assert_eq!(v.len(), 2);
        // requests 降序：A(2) 在前
        assert_eq!(v[0].model, "A");
        assert_eq!(v[0].requests, 2);
        assert_eq!(v[0].input_tokens, 30);
        assert_eq!(v[1].model, "B");
        assert_eq!(v[1].requests, 1);
    }

    // ===== aggregate_provider_breakdown =====

    #[test]
    fn aggregate_provider_breakdown_groups_by_db_type() {
        // db_type 非空 → 一级聚合按数据源归并（动态 provider 合并）
        let mut r1 = rec("s", "m", "prov-a", 0, 0, 0, 0, 0, 100);
        r1.db_type = "CCS".to_string();
        let mut r2 = rec("s", "m", "prov-b", 0, 0, 0, 0, 0, 300);
        r2.db_type = "CCS".to_string();
        let mut r3 = rec("s", "m", "DSH", 0, 0, 0, 0, 0, 200);
        r3.db_type = "DSH".to_string();
        // db_type 为空 → 回退 provider_id（兼容旧路径）
        let r4 = rec("s", "m", "legacy", 0, 0, 0, 0, 0, 50);

        let v = aggregate_provider_breakdown(&[r1, r2, r3, r4], &HashMap::new());
        assert_eq!(v.len(), 3);
        // CCS(2) 在前，两个动态 provider 归并为一行，名字即 canonical 名
        assert_eq!(v[0].provider_id, "CCS");
        assert_eq!(v[0].provider_name, "CCS");
        assert_eq!(v[0].requests, 2);
        assert!((v[0].avg_latency - 200.0).abs() < 1e-9);
        assert!((v[0].success_rate - 100.0).abs() < 1e-9);
        // DSH(1) 与 legacy(1) 请求数相同，排序稳定即可
        let ids: Vec<&str> = v.iter().map(|x| x.provider_id.as_str()).collect();
        assert!(ids.contains(&"DSH"));
        assert!(ids.contains(&"legacy"));
    }

    // ===== merge_provider_costs_to_db_type =====

    #[test]
    fn merge_provider_costs_to_db_type_groups_by_db_type() {
        // OpenCode 场景：provider_id ("opencode"/"anthropic") != db_type ("OpenCode") → 归并到 db_type
        let mut r1 = rec("s", "m", "opencode", 0, 100, 0, 0, 0, 0);
        r1.db_type = "OpenCode".to_string();
        let mut r2 = rec("s", "m", "anthropic", 0, 50, 0, 0, 0, 0);
        r2.db_type = "OpenCode".to_string();
        // db_type 为空 → 保留原 provider_id（与 breakdown 回退规则一致）
        let r3 = rec("s", "m", "legacy", 0, 10, 0, 0, 0, 0);

        let mut costs: HashMap<String, f64> = HashMap::new();
        costs.insert("opencode".to_string(), 1.5);
        costs.insert("anthropic".to_string(), 2.5);
        costs.insert("legacy".to_string(), 0.5);

        merge_provider_costs_to_db_type(&[r1, r2, r3], &mut costs);
        assert_eq!(costs.len(), 2);
        assert!((costs.get("OpenCode").unwrap() - 4.0).abs() < 1e-9);
        assert!((costs.get("legacy").unwrap() - 0.5).abs() < 1e-9);
        assert!(costs.get("opencode").is_none());
        assert!(costs.get("anthropic").is_none());
    }

    #[test]
    fn merge_provider_costs_to_db_type_keeps_unchanged_when_no_db_type() {
        let records = vec![rec("s", "m", "p1", 0, 1, 0, 0, 0, 0)];
        let mut costs: HashMap<String, f64> = HashMap::new();
        costs.insert("p1".to_string(), 3.0);
        merge_provider_costs_to_db_type(&records, &mut costs);
        assert_eq!(costs.len(), 1);
        assert!((costs.get("p1").unwrap() - 3.0).abs() < 1e-9);
    }

    // ===== aggregate_provider_model_tokens =====

    #[test]
    fn aggregate_provider_model_tokens_groups_pair() {
        let records = vec![
            rec("s", "A", "p1", 0, 100, 10, 5, 1, 0),
            rec("s", "A", "p1", 0, 200, 20, 5, 1, 0),
            rec("s", "A", "p2", 0, 50, 0, 0, 0, 0),
        ];
        let v = aggregate_provider_model_tokens(&records);
        assert_eq!(v.len(), 2);
        let p1 = v.iter().find(|x| x.provider_id == "p1").unwrap();
        assert_eq!(p1.input_tokens, 300);
        assert_eq!(p1.output_tokens, 30);
        assert_eq!(p1.cache_read, 10);
    }

    // ===== aggregate_daily_trend / hourly =====

    #[test]
    fn aggregate_daily_trend_groups_by_day_model() {
        let records = vec![
            rec("s", "A", "p", DAY1, 100, 0, 0, 0, 100),
            rec("s", "A", "p", DAY1, 200, 0, 0, 0, 200),
            rec("s", "A", "p", DAY1 + 86400, 50, 0, 0, 0, 300),
        ];
        let v = aggregate_daily_trend(&records, 0);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].day, "2024-01-01");
        assert_eq!(v[0].requests, 2);
        assert!((v[0].avg_latency - 150.0).abs() < 1e-9);
        assert_eq!(v[1].day, "2024-01-02");
    }

    #[test]
    fn aggregate_hourly_trend_groups_by_hour() {
        let records = vec![
            rec("s", "A", "p", DAY1, 10, 0, 0, 0, 0),        // 00:00
            rec("s", "A", "p", DAY1 + 3600, 20, 0, 0, 0, 0), // 01:00
        ];
        let v = aggregate_hourly_trend(&records, 0);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].day, "00:00");
        assert_eq!(v[1].day, "01:00");
    }

    // ===== aggregate_combined_records =====

    #[test]
    fn aggregate_combined_records_groups_and_sorts() {
        let records = vec![
            rec("s", "A", "p2", DAY1, 100, 0, 0, 0, 50),
            rec("s", "A", "p2", DAY1, 200, 0, 0, 0, 70),
            rec("s", "B", "p1", DAY1, 50, 0, 0, 0, 0),
        ];
        let v = aggregate_combined_records(&records, 0);
        assert_eq!(v.len(), 2);
        // 排序 (day, provider_id, model) 升序 → p1/B 在前
        assert_eq!(v[0].provider_id, "p1");
        assert_eq!(v[0].model, "B");
        assert_eq!(v[1].provider_id, "p2");
        assert_eq!(v[1].model, "A");
        assert_eq!(v[1].requests, 2);
        assert!((v[1].latency_sum - 120.0).abs() < 1e-9);
    }

    // ===== aggregate_session_breakdown =====

    #[test]
    fn aggregate_session_breakdown_skips_empty_and_tracks_bounds() {
        let records = vec![
            rec("s1", "A", "p", 300, 0, 0, 0, 0, 0),
            rec("s1", "A", "p", 100, 0, 0, 0, 0, 0),
            rec("", "A", "p", 500, 0, 0, 0, 0, 0), // 空 session，跳过
            rec("s2", "A", "p", 200, 0, 0, 0, 0, 0),
        ];
        let v = aggregate_session_breakdown(&records);
        assert_eq!(v.len(), 2);
        // requests 降序：s1(2) 在前
        assert_eq!(v[0].session_id, "s1");
        assert_eq!(v[0].requests, 2);
        assert_eq!(v[0].first_at, 100);
        assert_eq!(v[0].last_at, 300);
        assert_eq!(v[0].max_context_width, 0);
        assert_eq!(v[1].session_id, "s2");
    }

    // ===== aggregate_session_model_tokens =====

    #[test]
    fn aggregate_session_model_tokens_top_sessions() {
        // s1: 3 次, s2: 1 次（均在 top 500）
        let mut records = vec![];
        for _ in 0..3 {
            records.push(rec("s1", "A", "p", 0, 100, 10, 5, 1, 0));
        }
        records.push(rec("s2", "A", "p", 0, 50, 0, 0, 0, 0));
        let v = aggregate_session_model_tokens(&records);
        assert_eq!(v.len(), 2);
        let s1 = v.iter().find(|x| x.session_id == "s1").unwrap();
        assert_eq!(s1.input_tokens, 300);
        assert_eq!(s1.cache_read, 15);
    }

    // ===== aggregate_model_context_tier_buckets =====

    #[test]
    fn aggregate_model_context_tier_buckets_matches_and_merges() {
        // thresholds [10000, 50000]
        let records = vec![
            rec("s", "A", "p", 1000, 5000, 0, 0, 0, 0),  // ctx=5000 → tier 0
            rec("s", "A", "p", 2000, 12000, 0, 0, 0, 0), // ctx=12000 → tier 10000
            rec("s", "A", "p", 3000, 60000, 0, 0, 0, 0), // ctx=60000 → tier 50000
            rec("s", "A", "p", 1500, 60000, 0, 0, 0, 0), // ctx=60000 → tier 50000，合并
        ];
        let v = aggregate_model_context_tier_buckets(&records, 0, &[10000, 50000], None);
        assert_eq!(v.len(), 3); // tier: 0, 10000, 50000
        let t50k = v.iter().find(|x| x.context_tier == 50000).unwrap();
        assert_eq!(t50k.input_tokens, 120000); // 60000+60000
        // representative_epoch 取最小：min(3000,1500)=1500
        assert_eq!(t50k.representative_epoch, 1500);
        let t0 = v.iter().find(|x| x.context_tier == 0).unwrap();
        assert_eq!(t0.input_tokens, 5000);
    }

    #[test]
    fn aggregate_model_context_tier_buckets_empty_thresholds() {
        let records = vec![rec("s", "A", "p", 0, 1000, 0, 0, 0, 0)];
        assert!(aggregate_model_context_tier_buckets(&records, 0, &[], None).is_empty());
    }

    // ===== 数据源级过滤 (scope_to_provider_source) =====

    use crate::services::data_source::{DataSource, DbType};

    /// 最小假数据源：全部方法返回空默认值，仅供 scope 测试构造 SourceEntry。
    struct FakeSource;
    impl DataSource for FakeSource {
        fn open(&mut self, _path: &str) -> Result<(), String> { Ok(()) }
        fn close(&mut self) {}
        fn is_open(&self) -> bool { true }
        fn get_record_count(&self) -> Result<i64, String> { Ok(0) }
        fn get_latest_timestamp(&self) -> Option<i64> { None }
        fn get_providers(&self) -> Result<Vec<Provider>, String> { Ok(Vec::new()) }
        fn get_models(&self) -> Result<Vec<String>, String> { Ok(Vec::new()) }
        fn get_date_range(&self) -> Result<DateRange, String> { Ok(DateRange { min: 0, max: 0 }) }
        fn get_summary(&self, _params: &FilterParams) -> Result<SummaryData, String> {
            Ok(SummaryData { total_requests: 0, success_count: 0, total_input: 0, total_output: 0, total_cache_read: 0, total_cache_creation: 0, avg_latency: 0.0 })
        }
        fn get_model_breakdown(&self, _params: &FilterParams) -> Result<Vec<ModelBreakdown>, String> { Ok(Vec::new()) }
        fn get_provider_breakdown(&self, _params: &FilterParams) -> Result<Vec<ProviderBreakdown>, String> { Ok(Vec::new()) }
        fn get_combined_breakdown(&self, _params: &FilterParams) -> Result<Vec<CombinedBreakdownRow>, String> { Ok(Vec::new()) }
        fn get_provider_model_tokens(&self, _params: &FilterParams) -> Result<Vec<ProviderModelToken>, String> { Ok(Vec::new()) }
        fn get_daily_trend(&self, _params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> { Ok(Vec::new()) }
        fn get_hourly_trend(&self, _params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> { Ok(Vec::new()) }
        fn get_session_breakdown(&self, _params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> { Ok(Vec::new()) }
        fn get_session_max_context_widths(&self, _ids: &[String]) -> Result<HashMap<String, i64>, String> { Ok(HashMap::new()) }
        fn get_session_model_tokens(&self, _params: &FilterParams) -> Result<Vec<SessionModelToken>, String> { Ok(Vec::new()) }
        fn get_session_request_tokens(&self, _params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> { Ok(Vec::new()) }
        fn get_session_request_tokens_for_ids(&self, _params: &FilterParams, _session_ids: &[String]) -> Result<Vec<SessionRequestToken>, String> { Ok(Vec::new()) }
        fn get_session_model_tokens_for_ids(&self, _params: &FilterParams, _session_ids: &[String]) -> Result<Vec<SessionModelToken>, String> { Ok(Vec::new()) }
        fn get_session_timestamps(&self, _ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String> { Ok(HashMap::new()) }
        fn get_model_context_tier_buckets(&self, _params: &FilterParams, _thresholds: &[i64]) -> Result<Vec<ModelContextTierBucket>, String> { Ok(Vec::new()) }
        fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String> { Ok(Vec::new()) }
        fn get_recent_request_logs_raw(&self, _since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64, bool)>, String> { Ok(Vec::new()) }
        fn get_filtered_records(&self, _params: &FilterParams) -> Result<Vec<RawRecord>, String> { Ok(Vec::new()) }
    }

    fn src(db_type: DbType, enabled: bool) -> SourceEntry {
        SourceEntry {
            id: db_type.label().to_string(),
            path: String::new(),
            db_type,
            source: Box::new(FakeSource),
            enabled,
        }
    }

    fn params_with_provider(provider: Option<&str>) -> FilterParams {
        FilterParams {
            from_epoch: None,
            to_epoch: None,
            tz_offset: None,
            provider_id: provider.map(|p| p.to_string()),
            model_id: Some("m".to_string()),
            ccs_filter_session_apps: None,
        }
    }

    #[test]
    fn scope_to_provider_source_hit_keeps_only_source_and_drops_inner_filter() {
        let sources = vec![src(DbType::ExternalDb, true), src(DbType::OpenCode, true)];
        let (scoped, scoped_params) = scope_to_provider_source(&sources, &params_with_provider(Some("CCS")));
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].db_type, DbType::ExternalDb);
        // 内部 provider 过滤被剥离，其余筛选字段保留
        assert!(scoped_params.provider_id.is_none());
        assert_eq!(scoped_params.model_id.as_deref(), Some("m"));
    }

    #[test]
    fn scope_to_provider_source_unmatched_keeps_legacy_provider_filter() {
        // 历史内部 provider id（如 OpenCode providerID、CCS UUID）不匹配任何数据源名 → 保持原语义
        let sources = vec![src(DbType::ExternalDb, true), src(DbType::OpenCode, true)];
        let (scoped, scoped_params) = scope_to_provider_source(&sources, &params_with_provider(Some("anthropic")));
        assert_eq!(scoped.len(), 2);
        assert_eq!(scoped_params.provider_id.as_deref(), Some("anthropic"));
    }

    #[test]
    fn scope_to_provider_source_empty_keeps_all_sources() {
        let sources = vec![src(DbType::ExternalDb, true), src(DbType::OpenCode, true)];
        let (scoped, scoped_params) = scope_to_provider_source(&sources, &params_with_provider(None));
        assert_eq!(scoped.len(), 2);
        assert!(scoped_params.provider_id.is_none());
    }

    #[test]
    fn scope_to_provider_source_disabled_source_not_matched() {
        // 命中的目标源被禁用 → 退化为全量 + 原 params（由 fetch 的 enabled 过滤兜底）
        let sources = vec![src(DbType::ExternalDb, false), src(DbType::OpenCode, true)];
        let (scoped, scoped_params) = scope_to_provider_source(&sources, &params_with_provider(Some("CCS")));
        assert_eq!(scoped.len(), 2);
        assert_eq!(scoped_params.provider_id.as_deref(), Some("CCS"));
    }
}
