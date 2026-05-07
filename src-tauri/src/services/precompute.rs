use std::collections::HashMap;

use crate::models::*;
use crate::services::pricing_engine::PricingEngine;
use crate::utils::date_str_to_epoch;

// 一次遍历 dailyTrend + providerModelTokens，产出所有预计算结果
pub fn precompute_costs(
    daily_trend: &[DailyTrendRow],
    provider_model_tokens: &[ProviderModelToken],
    ps: &PricingEngine,
) -> PrecomputedResult {
    let mut model_costs: HashMap<String, f64> = HashMap::new();
    let mut model_cost_breakdown: HashMap<String, Vec<f64>> = HashMap::new();
    let mut provider_costs: HashMap<String, f64> = HashMap::new();
    let mut day_cost_map: HashMap<String, f64> = HashMap::new();
    let mut day_requests_map: HashMap<String, i64> = HashMap::new();
    let mut day_input_tokens: HashMap<String, i64> = HashMap::new();
    let mut day_output_tokens: HashMap<String, i64> = HashMap::new();
    let mut day_latency_sum: HashMap<String, f64> = HashMap::new();
    let mut day_latency_count: HashMap<String, i64> = HashMap::new();
    let mut daily_by_model: HashMap<String, Vec<DailyTrendRow>> = HashMap::new();

    for row in daily_trend {
        let epoch = date_str_to_epoch(&row.day);
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

    PrecomputedResult {
        model_costs,
        model_cost_breakdown,
        provider_costs,
        day_cost_map,
        day_requests_map,
        day_input_tokens,
        day_output_tokens,
        day_latency_sum,
        day_latency_count,
        daily_by_model,
        cache_durations: HashMap::new(),
        context_tier_costs: Vec::new(),
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
}

/// 全局上下文档位费用聚合（threshold → cost）
pub fn compute_global_context_tier_costs(
    requests: &[SessionRequestToken],
    ps: &PricingEngine,
) -> HashMap<i64, f64> {
    let mut tier_costs: HashMap<i64, f64> = HashMap::new();
    for req in requests {
        let context_size = req.input_tokens + req.cache_read;
        let pricing = match ps.get_pricing_at_with_context(&req.model, req.created_at, context_size) {
            Some(p) => p,
            None => continue,
        };
        let cost = ps.calculate_cost(
            &pricing,
            req.input_tokens,
            req.output_tokens,
            req.cache_read,
            req.cache_creation,
        );
        let tier_key = ps
            .get_matched_tier_threshold(&req.model, req.created_at, context_size)
            .unwrap_or(0);
        *tier_costs.entry(tier_key).or_insert(0.0) += cost;
    }
    tier_costs
}
