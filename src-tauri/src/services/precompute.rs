use std::collections::{HashMap, HashSet};

use crate::models::*;
use crate::services::pricing_engine::PricingEngine;
use crate::utils::date_str_to_epoch;

/// 将带时区的日期字符串转为 epoch 秒（本地零点 → UTC），无效日期返回 0
fn day_to_epoch_local(day: &str, tz_offset: i64) -> i64 {
    date_str_to_epoch(day).unwrap_or(0) - tz_offset * 3600
}

// 一次遍历 dailyTrend + providerModelTokens，产出所有预计算结果
pub fn precompute_costs(
    daily_trend: &[DailyTrendRow],
    provider_model_tokens: &[ProviderModelToken],
    ps: &PricingEngine,
    tz_offset: i64,
) -> PrecomputedResult {
    let mut model_costs: HashMap<String, f64> = HashMap::new();
    let mut model_cost_breakdown: HashMap<String, Vec<f64>> = HashMap::new();
    let mut provider_costs: HashMap<String, f64> = HashMap::new();
    let mut day_cost_map: HashMap<String, f64> = HashMap::new();
    let mut day_requests_map: HashMap<String, i64> = HashMap::new();
    let mut day_input_tokens: HashMap<String, i64> = HashMap::new();
    let mut day_output_tokens: HashMap<String, i64> = HashMap::new();
    let mut day_cache_read: HashMap<String, i64> = HashMap::new();
    let mut day_cache_creation: HashMap<String, i64> = HashMap::new();
    let mut day_latency_sum: HashMap<String, f64> = HashMap::new();
    let mut day_latency_count: HashMap<String, i64> = HashMap::new();
    let mut daily_by_model: HashMap<String, Vec<DailyTrendRow>> = HashMap::new();
    let mut unpriced_models_set: HashSet<String> = HashSet::new();

    for row in daily_trend {
        let epoch = day_to_epoch_local(&row.day, tz_offset);
        let p_at = ps.get_pricing_at(&row.model, epoch);
        let mut day_cost = 0.0f64;
        let mut day_cost_breakdown = vec![0.0f64, 0.0f64, 0.0f64, 0.0f64];

        if let Some(ref pricing) = p_at {
            day_cost_breakdown = ps.calculate_cost_breakdown(
                pricing,
                row.input_tokens,
                row.output_tokens,
                row.cache_read,
                row.cache_creation,
            );
            day_cost = day_cost_breakdown[0] + day_cost_breakdown[1] + day_cost_breakdown[2] + day_cost_breakdown[3];
        } else {
            unpriced_models_set.insert(row.model.clone());
        }

        // 累加模型费用
        *model_costs.entry(row.model.clone()).or_insert(0.0) += day_cost;

        // 累加模型费用分解
        let mb = model_cost_breakdown
            .entry(row.model.clone())
            .or_insert_with(|| vec![0.0, 0.0, 0.0, 0.0]);
        mb[0] += day_cost_breakdown[0];
        mb[1] += day_cost_breakdown[1];
        mb[2] += day_cost_breakdown[2];
        mb[3] += day_cost_breakdown[3];

        // 累加每日统计
        *day_cost_map.entry(row.day.clone()).or_insert(0.0) += day_cost;
        *day_requests_map.entry(row.day.clone()).or_insert(0) += row.requests;
        *day_input_tokens.entry(row.day.clone()).or_insert(0) += row.input_tokens;
        *day_output_tokens.entry(row.day.clone()).or_insert(0) += row.output_tokens;
        *day_cache_read.entry(row.day.clone()).or_insert(0) += row.cache_read;
        *day_cache_creation.entry(row.day.clone()).or_insert(0) += row.cache_creation;
        *day_latency_sum.entry(row.day.clone()).or_insert(0.0) += row.avg_latency * row.requests as f64;
        *day_latency_count.entry(row.day.clone()).or_insert(0) += row.requests;

        // 按模型分组每日数据
        daily_by_model
            .entry(row.model.clone())
            .or_default()
            .push(row.clone());
    }

    // 映射供应商费用：按 Token 比例将模型费用分配给供应商
    if !provider_model_tokens.is_empty() {
        let mut model_total_tokens_map: HashMap<String, i64> = HashMap::new();
        for pmt in provider_model_tokens {
            let total = pmt.input_tokens + pmt.output_tokens + pmt.cache_read + pmt.cache_creation;
            *model_total_tokens_map.entry(pmt.model.clone()).or_insert(0) += total;
        }
        for pmt in provider_model_tokens {
            let model_cost = model_costs.get(&pmt.model).copied().unwrap_or(0.0);
            if model_cost <= 0.0 {
                continue;
            }
            let pmt_tokens = pmt.input_tokens + pmt.output_tokens + pmt.cache_read + pmt.cache_creation;
            let total_tokens = model_total_tokens_map.get(&pmt.model).copied().unwrap_or(0);
            if total_tokens <= 0 || pmt_tokens <= 0 {
                continue;
            }
            *provider_costs.entry(pmt.provider_id.clone()).or_insert(0.0) +=
                model_cost * (pmt_tokens as f64 / total_tokens as f64);
        }
    }

    let mut unpriced_models: Vec<String> = unpriced_models_set.into_iter().collect();
    unpriced_models.sort();

    PrecomputedResult {
        model_costs,
        model_cost_breakdown,
        provider_costs,
        day_cost_map,
        day_requests_map,
        day_input_tokens,
        day_output_tokens,
        day_cache_read,
        day_cache_creation,
        day_latency_sum,
        day_latency_count,
        daily_by_model,
        model_context_tier_costs: HashMap::new(),
        model_compare_buckets: HashMap::new(),
        unpriced_models,
    }
}

// 计算会话费用（请求级时间感知定价）
pub fn compute_session_costs(
    session_request_tokens: &[SessionRequestToken],
    ps: &PricingEngine,
) -> HashMap<String, f64> {
    let mut session_costs: HashMap<String, f64> = HashMap::new();

    for req in session_request_tokens {
        let context_size = req.input_tokens + req.cache_read;
        if let Some(pricing) = ps.get_pricing_at_with_context(&req.model, req.created_at, context_size) {
            let cost = ps.calculate_cost(
                &pricing,
                req.input_tokens,
                req.output_tokens,
                req.cache_read,
                req.cache_creation,
            );
            *session_costs.entry(req.session_id.clone()).or_insert(0.0) += cost;
        }
    }

    session_costs
}

// 计算会话-模型费用分解
pub fn compute_session_model_costs(
    session_request_tokens: &[SessionRequestToken],
    session_model_tokens: &[SessionModelToken],
    ps: &PricingEngine,
) -> HashMap<String, HashMap<String, SessionModelCostData>> {
    let mut result: HashMap<String, HashMap<String, SessionModelCostData>> = HashMap::new();

    // 填入 Token 总量
    for smt in session_model_tokens {
        result
            .entry(smt.session_id.clone())
            .or_default()
            .insert(
                smt.model.clone(),
                SessionModelCostData {
                    cost: 0.0,
                    breakdown: vec![0.0, 0.0, 0.0, 0.0],
                    input_tokens: smt.input_tokens,
                    output_tokens: smt.output_tokens,
                    cache_read: smt.cache_read,
                    cache_creation: smt.cache_creation,
                    tier_costs: HashMap::new(),
                    tier_tokens: HashMap::new(),
                },
            );
    }

    // 从请求级数据累加费用
    for req in session_request_tokens {
        let context_size = req.input_tokens + req.cache_read;
        let pricing = match ps.get_pricing_at_with_context(&req.model, req.created_at, context_size) {
            Some(p) => p,
            None => continue,
        };
        let req_breakdown = ps.calculate_cost_breakdown(
            &pricing,
            req.input_tokens,
            req.output_tokens,
            req.cache_read,
            req.cache_creation,
        );
        let req_cost = req_breakdown[0] + req_breakdown[1] + req_breakdown[2] + req_breakdown[3];

        // 获取匹配的上下文档位 threshold（无匹配为 0）
        let tier_key = ps
            .get_matched_tier_threshold(&req.model, req.created_at, context_size)
            .unwrap_or(0);

        if let Some(session_map) = result.get_mut(&req.session_id) {
            if let Some(entry) = session_map.get_mut(&req.model) {
                entry.cost += req_cost;
                entry.breakdown[0] += req_breakdown[0];
                entry.breakdown[1] += req_breakdown[1];
                entry.breakdown[2] += req_breakdown[2];
                entry.breakdown[3] += req_breakdown[3];
                *entry.tier_costs.entry(tier_key).or_insert(0.0) += req_cost;
                let req_tokens = req.input_tokens + req.output_tokens + req.cache_read + req.cache_creation;
                *entry.tier_tokens.entry(tier_key).or_insert(0) += req_tokens;
            }
        }
    }

    result
}

pub struct SessionModelCostData {
    pub cost: f64,
    pub breakdown: Vec<f64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub tier_costs: HashMap<i64, f64>,
    pub tier_tokens: HashMap<i64, i64>,
}

/// 聚合结果，包含从 CombinedBreakdownRow 派生的三组数据
pub struct CombinedAggregation {
    pub model_breakdown: Vec<ModelBreakdown>,
    pub daily_trend: Vec<DailyTrendRow>,
    pub provider_model_tokens: Vec<ProviderModelToken>,
}

/// 从 CombinedBreakdownRow 列表一次性聚合出 model_breakdown、daily_trend、provider_model_tokens
pub fn aggregate_combined_breakdown(combined: &[CombinedBreakdownRow]) -> CombinedAggregation {
    let mut model_map: HashMap<String, (i64, i64, i64, i64, i64)> = HashMap::new();
    let mut daily_map: HashMap<(String, String), (i64, i64, i64, i64, i64, f64)> = HashMap::new();
    let mut pmt_map: HashMap<(String, String), (i64, i64, i64, i64)> = HashMap::new();

    for row in combined {
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

    let mut model_breakdown: Vec<ModelBreakdown> = model_map
        .into_iter()
        .map(|(model, (requests, input_tokens, output_tokens, cache_read, cache_creation))| {
            ModelBreakdown { model, requests, input_tokens, output_tokens, cache_read, cache_creation }
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

    let provider_model_tokens: Vec<ProviderModelToken> = pmt_map
        .into_iter()
        .map(|((provider_id, model), (input_tokens, output_tokens, cache_read, cache_creation))| {
            ProviderModelToken { provider_id, model, input_tokens, output_tokens, cache_read, cache_creation }
        })
        .collect();

    CombinedAggregation {
        model_breakdown,
        daily_trend,
        provider_model_tokens,
    }
}

/// 从 tier buckets 一次性计算 tier costs + 模型总费用（含四维分解）+ 每日费用
/// 等价于逐条请求调用 get_pricing_at_with_context 后求和
pub fn build_context_tier_and_model_costs(
    tier_buckets: &[crate::models::ModelContextTierBucket],
    ps: &PricingEngine,
) -> (HashMap<String, Vec<ContextTierCost>>, HashMap<String, f64>, HashMap<String, Vec<f64>>, HashMap<String, f64>) {
    let mut tier_costs_map: HashMap<String, Vec<ContextTierCost>> = HashMap::new();
    let mut model_costs: HashMap<String, f64> = HashMap::new();
    let mut model_breakdown: HashMap<String, Vec<f64>> = HashMap::new();
    let mut day_cost_map: HashMap<String, f64> = HashMap::new();

    for bucket in tier_buckets {
        let pricing_context = bucket.context_tier.max(0);
        let pricing = match ps.get_pricing_at_with_context(&bucket.model, bucket.representative_epoch, pricing_context) {
            Some(p) => p,
            None => continue,
        };
        let bd = ps.calculate_cost_breakdown(
            &pricing,
            bucket.input_tokens,
            bucket.output_tokens,
            bucket.cache_read,
            bucket.cache_creation,
        );
        let cost = bd[0] + bd[1] + bd[2] + bd[3];

        // 累加四维分解
        let e = model_breakdown.entry(bucket.model.clone()).or_insert_with(|| vec![0.0, 0.0, 0.0, 0.0]);
        e[0] += bd[0]; e[1] += bd[1]; e[2] += bd[2]; e[3] += bd[3];

        // 累加每日费用
        *day_cost_map.entry(bucket.day.clone()).or_insert(0.0) += cost;

        // 累加 tier costs
        let tier_key = ps
            .get_matched_tier_threshold(&bucket.model, bucket.representative_epoch, pricing_context)
            .unwrap_or(0);
        let tokens = bucket.input_tokens + bucket.output_tokens + bucket.cache_read + bucket.cache_creation;
        let entry = tier_costs_map
            .entry(bucket.model.clone())
            .or_default();
        match entry.iter_mut().find(|t| t.threshold == tier_key) {
            Some(existing) => { existing.cost += cost; existing.tokens += tokens; }
            None => entry.push(ContextTierCost { threshold: tier_key, cost, tokens }),
        }
    }

    // 模型总费用 = 四维分解之和
    for (model, bd) in &model_breakdown {
        model_costs.insert(model.clone(), bd[0] + bd[1] + bd[2] + bd[3]);
    }

    for tiers in tier_costs_map.values_mut() {
        tiers.sort_by_key(|t| t.threshold);
    }

    (tier_costs_map, model_costs, model_breakdown, day_cost_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::app_db::AppDbService;

    /// 构造带云端定价的引擎：claude-sonnet-4 单价 21/105/2.1/26.25（每百万 token）
    fn make_engine() -> PricingEngine {
        let app_db = AppDbService::new_in_memory().unwrap();
        app_db.save_cloud_pricing(&CloudPricingData {
            version: 1,
            updated_at: 1700000000,
            currency: "RMB".to_string(),
            models: vec![CloudPricingModel {
                model_id: "claude-sonnet-4".to_string(),
                input_cost_per_million: 21.0,
                output_cost_per_million: 105.0,
                cache_read_cost_per_million: 2.1,
                cache_creation_cost_per_million: 26.25,
                context_tiers: vec![],
                time_rules: vec![],
                aliases: vec![],
                no_cache_support: false,
            }],
        }).unwrap();
        let mut engine = PricingEngine::new();
        engine.refresh(&app_db).unwrap();
        engine
    }

    fn trend(day: &str, model: &str, requests: i64, input: i64, output: i64, cache_read: i64, cache_creation: i64) -> DailyTrendRow {
        DailyTrendRow {
            day: day.to_string(),
            model: model.to_string(),
            requests,
            input_tokens: input,
            output_tokens: output,
            cache_read,
            cache_creation,
            avg_latency: 0.0,
        }
    }

    // 空引擎：所有模型未定价，但仍应累加 token 与分组
    #[test]
    fn precompute_costs_unpriced_collects_models() {
        let ps = PricingEngine::new();
        let daily = vec![
            trend("2024-01-01", "unknown-a", 2, 100, 200, 50, 30),
            trend("2024-01-01", "unknown-b", 1, 10, 20, 5, 3),
        ];
        let r = precompute_costs(&daily, &[], &ps, 0);
        // 未定价模型收集（排序）
        assert_eq!(r.unpriced_models, vec!["unknown-a".to_string(), "unknown-b".to_string()]);
        // 费用全为 0
        assert!(r.model_costs.values().all(|&c| c == 0.0));
        assert!(r.day_cost_map.values().all(|&c| c == 0.0));
        // token 统计照常累加
        assert_eq!(r.day_requests_map.get("2024-01-01"), Some(&3));
        assert_eq!(r.day_input_tokens.get("2024-01-01"), Some(&110));
        assert_eq!(r.day_output_tokens.get("2024-01-01"), Some(&220));
        // daily_by_model 分组
        assert_eq!(r.daily_by_model.len(), 2);
        assert_eq!(r.daily_by_model.get("unknown-a").unwrap().len(), 1);
    }

    // 有定价：验证 model_costs / day_cost_map / 四维分解
    #[test]
    fn precompute_costs_priced_calculates_cost() {
        let ps = make_engine();
        let daily = vec![trend("2024-01-01", "claude-sonnet-4", 1, 1_000_000, 0, 0, 0)];
        let r = precompute_costs(&daily, &[], &ps, 0);
        // 仅 input 1M → 21.0
        let cost = *r.model_costs.get("claude-sonnet-4").unwrap();
        assert!((cost - 21.0).abs() < 1e-6);
        let day_cost = *r.day_cost_map.get("2024-01-01").unwrap();
        assert!((day_cost - 21.0).abs() < 1e-6);
        // breakdown[0] = input 费用，其余为 0
        let bd = r.model_cost_breakdown.get("claude-sonnet-4").unwrap();
        assert_eq!(bd.len(), 4);
        assert!((bd[0] - 21.0).abs() < 1e-6);
        assert!((bd[1]).abs() < 1e-9);
    }

    // 多维度费用累加（input/output/cache_read/cache_creation）
    #[test]
    fn precompute_costs_all_dimensions() {
        let ps = make_engine();
        let daily = vec![trend("2024-01-01", "claude-sonnet-4", 1, 1_000_000, 500_000, 200_000, 100_000)];
        let r = precompute_costs(&daily, &[], &ps, 0);
        let expected = (1_000_000.0 * 21.0 + 500_000.0 * 105.0 + 200_000.0 * 2.1 + 100_000.0 * 26.25) / 1_000_000.0;
        let cost = *r.model_costs.get("claude-sonnet-4").unwrap();
        assert!((cost - expected).abs() < 1e-6);
    }

    // provider_costs 按 token 比例分摊模型费用
    #[test]
    fn precompute_costs_provider_allocation_by_token_ratio() {
        let ps = make_engine();
        let daily = vec![trend("2024-01-01", "claude-sonnet-4", 2, 1_000_000, 0, 0, 0)];
        // 两个 provider 各贡献 1M token，模型总费用 21.0 → 各分 10.5
        let pmt = vec![
            ProviderModelToken { provider_id: "p1".into(), model: "claude-sonnet-4".into(), input_tokens: 1_000_000, output_tokens: 0, cache_read: 0, cache_creation: 0 },
            ProviderModelToken { provider_id: "p2".into(), model: "claude-sonnet-4".into(), input_tokens: 1_000_000, output_tokens: 0, cache_read: 0, cache_creation: 0 },
        ];
        let r = precompute_costs(&daily, &pmt, &ps, 0);
        assert!((r.provider_costs.get("p1").unwrap() - 10.5).abs() < 1e-6);
        assert!((r.provider_costs.get("p2").unwrap() - 10.5).abs() < 1e-6);
    }

    // aggregate_combined_breakdown：从 CombinedBreakdownRow 一次性聚合三组结果
    #[test]
    fn aggregate_combined_breakdown_aggregates_and_avg_latency() {
        let combined = vec![
            CombinedBreakdownRow { day: "2024-01-01".into(), provider_id: "p".into(), model: "A".into(), requests: 2, input_tokens: 100, output_tokens: 200, cache_read: 50, cache_creation: 30, latency_sum: 300.0 },
            CombinedBreakdownRow { day: "2024-01-02".into(), provider_id: "p".into(), model: "A".into(), requests: 1, input_tokens: 10, output_tokens: 20, cache_read: 5, cache_creation: 3, latency_sum: 100.0 },
        ];
        let agg = aggregate_combined_breakdown(&combined);
        // model_breakdown：A 合并 requests=3
        assert_eq!(agg.model_breakdown.len(), 1);
        assert_eq!(agg.model_breakdown[0].model, "A");
        assert_eq!(agg.model_breakdown[0].requests, 3);
        assert_eq!(agg.model_breakdown[0].input_tokens, 110);
        // daily_trend：两行，avg_latency = latency_sum / requests
        assert_eq!(agg.daily_trend.len(), 2);
        let d1 = agg.daily_trend.iter().find(|d| d.day == "2024-01-01").unwrap();
        assert!((d1.avg_latency - 150.0).abs() < 1e-9); // 300 / 2
        // provider_model_tokens：(p, A) 累加
        assert_eq!(agg.provider_model_tokens.len(), 1);
        assert_eq!(agg.provider_model_tokens[0].input_tokens, 110);
    }
}
