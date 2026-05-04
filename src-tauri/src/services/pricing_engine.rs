use std::collections::HashMap;

use crate::models::*;
use crate::services::app_db::AppDbService;
use crate::services::external_db::ExternalDbService;
use crate::utils::DEFAULT_EXCHANGE_RATE;

// 定价计算引擎 —— 三层定价优先级
pub struct PricingEngine {
    merged: HashMap<String, MergedPricing>,
    time_overrides: Vec<TimePricingRule>,
    time_overrides_by_model: HashMap<String, Vec<TimePricingRule>>,
    exchange_rate: f64,
}

impl PricingEngine {
    pub fn new() -> Self {
        Self {
            merged: HashMap::new(),
            time_overrides: Vec::new(),
            time_overrides_by_model: HashMap::new(),
            exchange_rate: DEFAULT_EXCHANGE_RATE,
        }
    }

    fn round(v: f64) -> f64 {
        (v * 1e6).round() / 1e6
    }

    // 刷新全部定价数据
    pub fn refresh(
        &mut self,
        external_db: &ExternalDbService,
        app_db: &AppDbService,
    ) -> Result<(), String> {
        // 1. 加载汇率
        self.exchange_rate = app_db.get_exchange_rate();

        // 2. 加载基础定价（外部 DB 可能尚未打开）
        let base = if external_db.is_open() {
            external_db.get_base_pricing()?
        } else {
            Vec::new()
        };

        // 3. 加载用户覆盖
        let overrides = app_db.get_all_overrides()?;

        // 4. 合并
        self.merge(base, overrides);

        // 5. 加载时间规则并分组
        self.time_overrides = app_db.get_all_time_overrides()?;
        self.time_overrides_by_model.clear();
        for rule in &self.time_overrides {
            self.time_overrides_by_model
                .entry(rule.model_id.clone())
                .or_default()
                .push(rule.clone());
        }

        Ok(())
    }

    fn merge(&mut self, base: Vec<ModelPricing>, overrides: Vec<PricingOverride>) {
        let mut result: HashMap<String, MergedPricing> = HashMap::new();

        // 基础定价 × 汇率
        for bp in base {
            result.insert(
                bp.model_id.clone(),
                MergedPricing {
                    display_name: bp.display_name.clone(),
                    model_id: bp.model_id,
                    input_cost_per_million: Self::round(bp.input_cost_per_million * self.exchange_rate),
                    output_cost_per_million: Self::round(bp.output_cost_per_million * self.exchange_rate),
                    cache_read_cost_per_million: Self::round(bp.cache_read_cost_per_million * self.exchange_rate),
                    cache_creation_cost_per_million: Self::round(bp.cache_creation_cost_per_million * self.exchange_rate),
                    is_override: false,
                },
            );
        }

        // 覆盖替换基础
        for ov in overrides {
            let display_name = result
                .get(&ov.model_id)
                .map(|p| p.display_name.clone())
                .unwrap_or_else(|| ov.model_id.clone());
            result.insert(
                ov.model_id.clone(),
                MergedPricing {
                    model_id: ov.model_id,
                    display_name,
                    input_cost_per_million: ov.input_cost_per_million,
                    output_cost_per_million: ov.output_cost_per_million,
                    cache_read_cost_per_million: ov.cache_read_cost_per_million,
                    cache_creation_cost_per_million: ov.cache_creation_cost_per_million,
                    is_override: true,
                },
            );
        }

        self.merged = result;
    }

    // 获取固定合并定价
    pub fn get_pricing(&self, model_id: &str) -> Option<&MergedPricing> {
        self.merged.get(model_id)
    }

    // 时间感知定价查询：优先时间规则 → 固定定价
    pub fn get_pricing_at(&self, model_id: &str, epoch_seconds: i64) -> Option<MergedPricing> {
        if let Some(rules) = self.time_overrides_by_model.get(model_id) {
            for rule in rules {
                if rule.start_time <= epoch_seconds && epoch_seconds <= rule.end_time {
                    let base_entry = self.merged.get(model_id);
                    return Some(MergedPricing {
                        model_id: model_id.to_string(),
                        display_name: base_entry
                            .map(|p| p.display_name.clone())
                            .unwrap_or_else(|| model_id.to_string()),
                        input_cost_per_million: rule.input_cost_per_million,
                        output_cost_per_million: rule.output_cost_per_million,
                        cache_read_cost_per_million: rule.cache_read_cost_per_million,
                        cache_creation_cost_per_million: rule.cache_creation_cost_per_million,
                        is_override: true,
                    });
                }
            }
        }
        self.merged.get(model_id).cloned()
    }

    // 费用计算
    pub fn calculate_cost(&self, pricing: &MergedPricing, input: i64, output: i64, cache_read: i64, cache_creation: i64) -> f64 {
        (input as f64 * pricing.input_cost_per_million
            + output as f64 * pricing.output_cost_per_million
            + cache_read as f64 * pricing.cache_read_cost_per_million
            + cache_creation as f64 * pricing.cache_creation_cost_per_million)
            / 1_000_000.0
    }

    // 四维费用分解
    pub fn calculate_cost_breakdown(
        &self,
        pricing: &MergedPricing,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_creation: i64,
    ) -> Vec<f64> {
        vec![
            input as f64 * pricing.input_cost_per_million / 1_000_000.0,
            output as f64 * pricing.output_cost_per_million / 1_000_000.0,
            cache_read as f64 * pricing.cache_read_cost_per_million / 1_000_000.0,
            cache_creation as f64 * pricing.cache_creation_cost_per_million / 1_000_000.0,
        ]
    }

    pub fn get_exchange_rate(&self) -> f64 {
        self.exchange_rate
    }

    pub fn get_all_pricing(&self) -> Vec<MergedPricing> {
        self.merged.values().cloned().collect()
    }

    pub fn get_time_rules(&self, model_id: &str) -> Vec<TimePricingRule> {
        self.time_overrides_by_model
            .get(model_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn has_time_pricing(&self, model_id: &str) -> bool {
        self.time_overrides_by_model
            .get(model_id)
            .map(|r| !r.is_empty())
            .unwrap_or(false)
    }

    pub fn size(&self) -> usize {
        self.merged.len()
    }
}
