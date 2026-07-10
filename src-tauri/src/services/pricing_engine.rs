use std::collections::HashMap;

use crate::models::*;
use crate::services::app_db::AppDbService;
use crate::services::cloud_pricing;
use crate::utils::CLOUD_PRICING_URL;

/// 提供时间范围访问的 trait，用于统一匹配用户时间规则和云端时间规则
trait HasTimeRange {
    fn time_range(&self) -> (i64, i64);
}

impl HasTimeRange for TimePricingRule {
    fn time_range(&self) -> (i64, i64) {
        (self.start_time, self.end_time)
    }
}

impl HasTimeRange for CloudPricingTimeRule {
    fn time_range(&self) -> (i64, i64) {
        (self.start_time, self.end_time)
    }
}

// 定价计算引擎 —— 四层定价优先级（上下文大小为子维度）
pub struct PricingEngine {
    merged: HashMap<String, MergedPricing>,
    time_overrides: Vec<TimePricingRule>,
    time_overrides_by_model: HashMap<String, Vec<TimePricingRule>>,
    cloud_time_rules_by_model: HashMap<String, Vec<CloudPricingTimeRule>>,
    override_tiers: HashMap<String, Vec<ContextTier>>,
    time_rule_tiers: HashMap<i64, Vec<ContextTier>>,
    cloud_time_rule_tiers: HashMap<(String, i64, i64), Vec<ContextTier>>,
    alias_to_model_id: HashMap<String, String>,
    model_aliases: HashMap<String, Vec<String>>,
    no_cache_models: std::collections::HashSet<String>,
    model_families: HashMap<String, String>,
    families: Vec<PricingFamily>,
}

impl PricingEngine {
    pub fn new() -> Self {
        Self {
            merged: HashMap::new(),
            time_overrides: Vec::new(),
            time_overrides_by_model: HashMap::new(),
            cloud_time_rules_by_model: HashMap::new(),
            override_tiers: HashMap::new(),
            time_rule_tiers: HashMap::new(),
            cloud_time_rule_tiers: HashMap::new(),
            alias_to_model_id: HashMap::new(),
            model_aliases: HashMap::new(),
            no_cache_models: std::collections::HashSet::new(),
            model_families: HashMap::new(),
            families: Vec::new(),
        }
    }

    fn resolve_tier(tiers: &[ContextTier], context_size: i64) -> Option<&ContextTier> {
        let mut best: Option<&ContextTier> = None;
        for tier in tiers {
            if tier.threshold <= context_size {
                best = Some(tier);
            } else {
                break;
            }
        }
        best
    }

    fn tier_to_merged(model_id: &str, tier: &ContextTier, is_override: bool) -> MergedPricing {
        MergedPricing {
            model_id: model_id.to_string(),
            input_cost_per_million: tier.input_cost_per_million,
            output_cost_per_million: tier.output_cost_per_million,
            cache_read_cost_per_million: tier.cache_read_cost_per_million,
            cache_creation_cost_per_million: tier.cache_creation_cost_per_million,
            is_override,
        }
    }

    /// 在规则列表中查找匹配的时间规则（第一条时间范围命中的规则）
    fn find_matching_rule<'a, T: HasTimeRange>(
        rules: &'a [T],
        epoch_seconds: i64,
    ) -> Option<&'a T> {
        for rule in rules {
            let (start, end) = rule.time_range();
            if start <= epoch_seconds && epoch_seconds <= end {
                return Some(rule);
            }
        }
        None
    }

    // 刷新全部定价数据
    pub fn refresh(&mut self, app_db: &AppDbService) -> Result<(), String> {
        // 1. 加载云端定价：优先在线拉取，失败则读缓存
        let (base, cloud_tiers, cloud_time_rules, cloud_aliases, no_cache_models, model_families, families) = self.load_cloud_base(app_db);
        self.model_families = model_families;
        self.families = families;

        // 2. 先构建云端别名反向映射（统一小写 key），供 merge 和 resolve 使用
        self.alias_to_model_id.clear();
        self.model_aliases.clear();
        for (model_id, aliases) in &cloud_aliases {
            // modelId 本身也加入映射（大小写不敏感）
            self.alias_to_model_id.insert(model_id.to_lowercase(), model_id.clone());
            self.model_aliases.insert(model_id.clone(), aliases.clone());
            for alias in aliases {
                self.alias_to_model_id.insert(alias.to_lowercase(), model_id.clone());
            }
        }
        if let Ok(user_aliases) = app_db.get_user_aliases() {
            for (model_id, aliases) in user_aliases {
                let existing = self.model_aliases.get(&model_id).cloned().unwrap_or_default();
                self.model_aliases.insert(model_id.clone(), [&existing[..], &aliases[..]].concat());
                for alias in aliases {
                    self.alias_to_model_id.insert(alias.to_lowercase(), model_id.clone());
                }
            }
        }

        // 3. 加载用户覆盖
        let overrides = app_db.get_all_overrides()?;

        // 4. 合并 + 提取覆盖上下文档位（override model_id 通过别名映射解析）
        self.override_tiers.clear();
        self.no_cache_models = no_cache_models.into_iter().collect();
        self.merge(base, cloud_tiers, overrides);

        // 5. 加载云端时间规则（已按 model_id 分组）+ 排序上下文档位
        self.cloud_time_rule_tiers.clear();
        self.cloud_time_rules_by_model = cloud_time_rules;
        for rules in self.cloud_time_rules_by_model.values_mut() {
            for rule in rules {
                if !rule.context_tiers.is_empty() {
                    rule.context_tiers.sort_by_key(|t| t.threshold);
                    self.cloud_time_rule_tiers.insert(
                        (rule.model_id.clone(), rule.start_time, rule.end_time),
                        rule.context_tiers.clone(),
                    );
                }
            }
        }

        // 5. 加载用户时间规则并分组 + 提取时间规则上下文档位
        self.time_rule_tiers.clear();
        self.time_overrides = app_db.get_all_time_overrides()?;
        self.time_overrides_by_model.clear();
        for rule in &self.time_overrides {
            if !rule.context_tiers.is_empty() {
                self.time_rule_tiers.insert(rule.id, rule.context_tiers.clone());
            }
            self.time_overrides_by_model
                .entry(rule.model_id.clone())
                .or_default()
                .push(rule.clone());
        }
        // 按 id 降序：后创建的规则优先匹配
        for rules in self.time_overrides_by_model.values_mut() {
            rules.sort_by(|a, b| b.id.cmp(&a.id));
        }

        Ok(())
    }

    /// 从本地缓存加载云端基础定价（无网络请求）
    fn load_cloud_base(&self, app_db: &AppDbService) -> (Vec<ModelPricing>, HashMap<String, Vec<ContextTier>>, HashMap<String, Vec<CloudPricingTimeRule>>, HashMap<String, Vec<String>>, Vec<String>, HashMap<String, String>, Vec<PricingFamily>) {
        match app_db.load_cloud_pricing() {
            Ok((base, tiers, cloud_time_rules, cloud_aliases, no_cache_models, model_families)) => {
                let families = app_db.load_cloud_families().unwrap_or_default();
                let time_rule_count: usize = cloud_time_rules.values().map(|v| v.len()).sum();
                log::info!("[PRICING] 从缓存加载云端定价: {} 个模型, {} 条时间规则, {} 个无缓存模型", base.len(), time_rule_count, no_cache_models.len());
                (base, tiers, cloud_time_rules, cloud_aliases, no_cache_models, model_families, families)
            }
            Err(e) => {
                log::error!("[PRICING] 读取云端定价缓存失败: {}", e);
                (Vec::new(), HashMap::new(), HashMap::new(), HashMap::new(), Vec::new(), HashMap::new(), Vec::new())
            }
        }
    }

    /// 从网络拉取云端定价，比较版本后写入缓存
    pub fn fetch_and_cache_cloud_pricing(&self, app_db: &AppDbService) -> Result<(), String> {
        let data = cloud_pricing::fetch_cloud_pricing(CLOUD_PRICING_URL)?;
        log::info!("[PRICING] 云端定价拉取成功: {} 个模型, version={}", data.models.len(), data.version);
        let cached_version = app_db.get_setting("cloud_pricing_version");
        if cached_version.as_deref() != Some(&data.version.to_string()) {
            log::info!("[PRICING] 版本变化 {} → {}, 更新缓存", cached_version.unwrap_or_default(), data.version);
            app_db.save_cloud_pricing(&data)?;
        } else {
            log::info!("[PRICING] 版本未变化 ({}), 跳过缓存更新", data.version);
        }
        Ok(())
    }

    fn merge(&mut self, base: Vec<ModelPricing>, cloud_tiers: HashMap<String, Vec<ContextTier>>, overrides: Vec<PricingOverride>) {
        let mut result: HashMap<String, MergedPricing> = HashMap::new();

        // 云端基础定价（已是 RMB，无需汇率转换）
        for bp in base {
            result.insert(
                bp.model_id.clone(),
                MergedPricing {
                    model_id: bp.model_id,
                    input_cost_per_million: bp.input_cost_per_million,
                    output_cost_per_million: bp.output_cost_per_million,
                    cache_read_cost_per_million: bp.cache_read_cost_per_million,
                    cache_creation_cost_per_million: bp.cache_creation_cost_per_million,
                    is_override: false,
                },
            );
        }

        // 云端基础上下文档位
        for (model_id, tiers) in cloud_tiers {
            if !result.contains_key(&model_id) {
                continue;
            }
            if !self.override_tiers.contains_key(&model_id) {
                self.override_tiers.insert(model_id, tiers);
            }
        }

        // 用户覆盖替换基础（通过别名映射解析旧 model_id）
        for ov in overrides {
            let resolved = self.resolve_model_id(&ov.model_id)
                .unwrap_or_else(|| ov.model_id.clone());
            if !ov.context_tiers.is_empty() {
                let mut sorted = ov.context_tiers;
                sorted.sort_by_key(|t| t.threshold);
                self.override_tiers.insert(resolved.clone(), sorted);
            }
            result.insert(
                resolved.clone(),
                MergedPricing {
                    model_id: resolved,
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

    // 别名/modelId → modelId 解析（大小写不敏感）
    pub fn resolve_model_id(&self, raw_model_id: &str) -> Option<String> {
        self.alias_to_model_id.get(&raw_model_id.to_lowercase()).cloned()
    }

    // 获取某模型的所有别名（云端 + 用户）
    pub fn get_aliases(&self, model_id: &str) -> Vec<String> {
        self.model_aliases.get(model_id).cloned().unwrap_or_default()
    }

    pub fn get_no_cache_support(&self, model_id: &str) -> bool {
        self.no_cache_models.contains(model_id)
    }

    pub fn get_family(&self, model_id: &str) -> String {
        self.model_families
            .get(model_id)
            .cloned()
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| "other".to_string())
    }

    pub fn get_families(&self) -> &[PricingFamily] {
        &self.families
    }

    // 获取固定合并定价
    #[cfg(test)]
    pub fn get_pricing(&self, model_id: &str) -> Option<&MergedPricing> {
        let resolved = self.resolve_model_id(model_id)?;
        self.merged.get(&resolved)
    }

    // 时间感知定价查询：用户时间规则 → 云端时间规则 → 固定定价
    pub fn get_pricing_at(&self, model_id: &str, epoch_seconds: i64) -> Option<MergedPricing> {
        let resolved = match self.resolve_model_id(model_id) {
            Some(r) => r,
            None => return None,
        };

        // 1. 用户自定义时间规则
        if let Some(rules) = self.time_overrides_by_model.get(&resolved) {
            if let Some(rule) = Self::find_matching_rule(rules, epoch_seconds) {
                return Some(MergedPricing {
                    model_id: resolved.clone(),
                    input_cost_per_million: rule.input_cost_per_million,
                    output_cost_per_million: rule.output_cost_per_million,
                    cache_read_cost_per_million: rule.cache_read_cost_per_million,
                    cache_creation_cost_per_million: rule.cache_creation_cost_per_million,
                    is_override: true,
                });
            }
        }
        // 2. 云端时间规则
        if let Some(rules) = self.cloud_time_rules_by_model.get(&resolved) {
            if let Some(rule) = Self::find_matching_rule(rules, epoch_seconds) {
                return Some(MergedPricing {
                    model_id: resolved.clone(),
                    input_cost_per_million: rule.input_cost_per_million,
                    output_cost_per_million: rule.output_cost_per_million,
                    cache_read_cost_per_million: rule.cache_read_cost_per_million,
                    cache_creation_cost_per_million: rule.cache_creation_cost_per_million,
                    is_override: false,
                });
            }
        }
        self.merged.get(&resolved).cloned()
    }

    // 上下文感知定价查询：用户时间规则 → 云端时间规则 → 覆盖档位 → 固定定价
    pub fn get_pricing_at_with_context(
        &self,
        model_id: &str,
        epoch_seconds: i64,
        context_size: i64,
    ) -> Option<MergedPricing> {
        let resolved = match self.resolve_model_id(model_id) {
            Some(r) => r.to_string(),
            None => return None,
        };

        // 1. 用户自定义时间规则
        if let Some(rules) = self.time_overrides_by_model.get(&resolved) {
            if let Some(rule) = Self::find_matching_rule(rules, epoch_seconds) {
                if let Some(tiers) = self.time_rule_tiers.get(&rule.id) {
                    if let Some(tier) = Self::resolve_tier(tiers, context_size) {
                        return Some(Self::tier_to_merged(&resolved, tier, true));
                    }
                }
                return Some(MergedPricing {
                    model_id: resolved.clone(),
                    input_cost_per_million: rule.input_cost_per_million,
                    output_cost_per_million: rule.output_cost_per_million,
                    cache_read_cost_per_million: rule.cache_read_cost_per_million,
                    cache_creation_cost_per_million: rule.cache_creation_cost_per_million,
                    is_override: true,
                });
            }
        }

        // 2. 云端时间规则
        if let Some(rules) = self.cloud_time_rules_by_model.get(&resolved) {
            if let Some(rule) = Self::find_matching_rule(rules, epoch_seconds) {
                if let Some(tiers) = self.cloud_time_rule_tiers.get(&(resolved.clone(), rule.start_time, rule.end_time)) {
                    if let Some(tier) = Self::resolve_tier(tiers, context_size) {
                        return Some(Self::tier_to_merged(&resolved, tier, false));
                    }
                }
                return Some(MergedPricing {
                    model_id: resolved.clone(),
                    input_cost_per_million: rule.input_cost_per_million,
                    output_cost_per_million: rule.output_cost_per_million,
                    cache_read_cost_per_million: rule.cache_read_cost_per_million,
                    cache_creation_cost_per_million: rule.cache_creation_cost_per_million,
                    is_override: false,
                });
            }
        }

        // 3. 覆盖上下文档位
        if let Some(tiers) = self.override_tiers.get(&resolved) {
            if let Some(tier) = Self::resolve_tier(tiers, context_size) {
                return Some(Self::tier_to_merged(&resolved, tier, true));
            }
        }

        // 4. 固定定价
        self.merged.get(&resolved).cloned()
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

    pub fn get_all_pricing(&self) -> Vec<MergedPricing> {
        self.merged.values().cloned().collect()
    }

    pub fn get_time_rules(&self, model_id: &str) -> Vec<TimePricingRule> {
        let resolved = match self.resolve_model_id(model_id) {
            Some(r) => r,
            None => return Vec::new(),
        };
        self.time_overrides_by_model
            .get(&resolved)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_cloud_time_rules(&self, model_id: &str) -> Vec<CloudPricingTimeRule> {
        let resolved = match self.resolve_model_id(model_id) {
            Some(r) => r,
            None => return Vec::new(),
        };
        self.cloud_time_rules_by_model
            .get(&resolved)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_override_tiers(&self, model_id: &str) -> Vec<ContextTier> {
        let resolved = match self.resolve_model_id(model_id) {
            Some(r) => r,
            None => return Vec::new(),
        };
        self.override_tiers.get(&resolved).cloned().unwrap_or_default()
    }

    /// 返回命中的上下文档位 threshold，无命中返回 None
    pub fn get_matched_tier_threshold(&self, model_id: &str, epoch_seconds: i64, context_size: i64) -> Option<i64> {
        let resolved = match self.resolve_model_id(model_id) {
            Some(r) => r.to_string(),
            None => return None,
        };
        // 1. 用户时间规则的档位
        if let Some(rules) = self.time_overrides_by_model.get(&resolved) {
            if let Some(rule) = Self::find_matching_rule(rules, epoch_seconds) {
                if let Some(tiers) = self.time_rule_tiers.get(&rule.id) {
                    if let Some(tier) = Self::resolve_tier(tiers, context_size) {
                        return Some(tier.threshold);
                    }
                }
                return None;
            }
        }
        // 2. 云端时间规则的档位
        if let Some(rules) = self.cloud_time_rules_by_model.get(&resolved) {
            if let Some(rule) = Self::find_matching_rule(rules, epoch_seconds) {
                if let Some(tiers) = self.cloud_time_rule_tiers.get(&(resolved.clone(), rule.start_time, rule.end_time)) {
                    if let Some(tier) = Self::resolve_tier(tiers, context_size) {
                        return Some(tier.threshold);
                    }
                }
                return None;
            }
        }
        // 3. 覆盖档位
        if let Some(tiers) = self.override_tiers.get(&resolved) {
            if let Some(tier) = Self::resolve_tier(tiers, context_size) {
                return Some(tier.threshold);
            }
        }
        None
    }

    pub fn has_time_pricing(&self, model_id: &str) -> bool {
        let resolved = match self.resolve_model_id(model_id) {
            Some(r) => r,
            None => return false,
        };
        self.time_overrides_by_model
            .get(&resolved)
            .map(|r| !r.is_empty())
            .unwrap_or(false)
            || self.cloud_time_rules_by_model
                .get(&resolved)
                .map(|r| !r.is_empty())
                .unwrap_or(false)
    }

    pub fn size(&self) -> usize {
        self.merged.len()
    }

    /// 收集所有已知的上下文档位阈值，用于 SQL 端预聚合
    pub fn get_all_tier_thresholds(&self) -> Vec<i64> {
        let mut thresholds: Vec<i64> = self.override_tiers.values()
            .flat_map(|tiers| tiers.iter().map(|t| t.threshold))
            .chain(self.time_rule_tiers.values()
                .flat_map(|tiers| tiers.iter().map(|t| t.threshold)))
            .chain(self.cloud_time_rule_tiers.values()
                .flat_map(|tiers| tiers.iter().map(|t| t.threshold)))
            .collect();
        thresholds.sort();
        thresholds.dedup();
        thresholds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::app_db::AppDbService;

    fn create_test_engine() -> (PricingEngine, AppDbService) {
        let app_db = AppDbService::new_in_memory().unwrap();
        // 插入云端基础定价
        app_db.save_cloud_pricing(&CloudPricingData {
            version: 1,
            updated_at: 1700000000,
            currency: "RMB".to_string(),
            families: vec![],
            models: vec![
                CloudPricingModel {
                    model_id: "claude-sonnet-4".to_string(),
                    input_cost_per_million: 21.0,
                    output_cost_per_million: 105.0,
                    cache_read_cost_per_million: 2.1,
                    cache_creation_cost_per_million: 26.25,
                    context_tiers: vec![ContextTier {
                        id: None,
                        threshold: 10000,
                        input_cost_per_million: 31.5,
                        output_cost_per_million: 157.5,
                        cache_read_cost_per_million: 3.15,
                        cache_creation_cost_per_million: 39.375,
                    }],
                    time_rules: vec![],
                    aliases: vec!["claude-4-sonnet".to_string()],
                    no_cache_support: false,
                    family: "claude".to_string(),
                },
                CloudPricingModel {
                    model_id: "claude-haiku-4".to_string(),
                    input_cost_per_million: 4.2,
                    output_cost_per_million: 21.0,
                    cache_read_cost_per_million: 0.42,
                    cache_creation_cost_per_million: 5.25,
                    context_tiers: vec![],
                    time_rules: vec![],
                    aliases: vec![],
                    no_cache_support: false,
                    family: "claude".to_string(),
                },
            ],
        }).unwrap();
        let mut engine = PricingEngine::new();
        engine.refresh(&app_db).unwrap();
        (engine, app_db)
    }

    #[test]
    fn test_calculate_cost() {
        let (engine, _) = create_test_engine();
        let pricing = engine.get_pricing("claude-sonnet-4").unwrap();
        let cost = engine.calculate_cost(pricing, 1_000_000, 0, 0, 0);
        assert!((cost - 21.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_all_dims() {
        let (engine, _) = create_test_engine();
        let pricing = engine.get_pricing("claude-sonnet-4").unwrap();
        let cost = engine.calculate_cost(pricing, 1_000_000, 500_000, 200_000, 100_000);
        let expected = (1_000_000.0 * 21.0 + 500_000.0 * 105.0 + 200_000.0 * 2.1 + 100_000.0 * 26.25) / 1_000_000.0;
        assert!((cost - expected).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_zero() {
        let (engine, _) = create_test_engine();
        let pricing = engine.get_pricing("claude-sonnet-4").unwrap();
        let cost = engine.calculate_cost(pricing, 0, 0, 0, 0);
        assert!((cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_breakdown() {
        let (engine, _) = create_test_engine();
        let pricing = engine.get_pricing("claude-sonnet-4").unwrap();
        let bd = engine.calculate_cost_breakdown(pricing, 1_000_000, 0, 0, 0);
        assert!((bd[0] - 21.0).abs() < 0.001);
        assert!((bd[1] - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_get_pricing_cloud_base() {
        let (engine, _) = create_test_engine();
        let p = engine.get_pricing("claude-sonnet-4").unwrap();
        assert_eq!(p.model_id, "claude-sonnet-4");
        assert!(!p.is_override);
        assert!((p.input_cost_per_million - 21.0).abs() < 0.001);
    }

    #[test]
    fn test_get_pricing_unknown() {
        let (engine, _) = create_test_engine();
        assert!(engine.get_pricing("unknown").is_none());
    }

    #[test]
    fn test_get_pricing_at_with_user_time_rule() {
        let (mut engine, app_db) = create_test_engine();
        // 添加用户时间规则
        app_db.add_time_override(
            "claude-sonnet-4", 1700000000, 1700086400,
            10.5, 52.5, 1.05, 13.125, "折扣"
        ).unwrap();
        engine.refresh(&app_db).unwrap();

        let p = engine.get_pricing_at("claude-sonnet-4", 1700043200).unwrap();
        assert!(p.is_override);
        assert!((p.input_cost_per_million - 10.5).abs() < 0.001);
    }

    #[test]
    fn test_get_pricing_at_newer_time_rule_priority() {
        let (mut engine, app_db) = create_test_engine();
        // 旧规则（id 小，范围 1700000000 ~ 1700086400）
        app_db.add_time_override(
            "claude-sonnet-4", 1700000000, 1700086400,
            10.5, 52.5, 1.05, 13.125, "旧折扣"
        ).unwrap();
        // 新规则（id 大，范围 1700040000 ~ 1700086400，与旧规则重叠）
        app_db.add_time_override(
            "claude-sonnet-4", 1700040000, 1700086400,
            7.0, 35.0, 0.7, 8.75, "新折扣"
        ).unwrap();
        engine.refresh(&app_db).unwrap();

        // 1700050000 同时落在两条规则范围内
        let p = engine.get_pricing_at("claude-sonnet-4", 1700050000).unwrap();
        assert!(p.is_override);
        // 后创建的规则优先（新折扣 7.0 而非旧折扣 10.5）
        assert!((p.input_cost_per_million - 7.0).abs() < 0.001);

        // 1700030000 只落在旧规则范围内，不受影响
        let p2 = engine.get_pricing_at("claude-sonnet-4", 1700030000).unwrap();
        assert!((p2.input_cost_per_million - 10.5).abs() < 0.001);
    }

    #[test]
    fn test_get_pricing_at_fallback() {
        let (engine, _) = create_test_engine();
        let p = engine.get_pricing_at("claude-sonnet-4", 9999999999).unwrap();
        assert!(!p.is_override);
        assert!((p.input_cost_per_million - 21.0).abs() < 0.001);
    }

    #[test]
    fn test_get_pricing_at_with_context_tier() {
        let (engine, _) = create_test_engine();
        // 云端基础有 tier at threshold=10000
        let p = engine.get_pricing_at_with_context("claude-sonnet-4", 9999999999, 30000).unwrap();
        assert!(p.is_override);
        assert!((p.input_cost_per_million - 31.5).abs() < 0.001);
    }

    #[test]
    fn test_get_pricing_at_with_context_below_tier() {
        let (engine, _) = create_test_engine();
        let p = engine.get_pricing_at_with_context("claude-sonnet-4", 9999999999, 5000).unwrap();
        assert!(!p.is_override);
        assert!((p.input_cost_per_million - 21.0).abs() < 0.001);
    }

    #[test]
    fn test_resolve_tier_empty() {
        let result = PricingEngine::resolve_tier(&[], 1000);
        assert!(result.is_none());
    }

    #[test]
    fn test_has_time_pricing() {
        let (engine, _) = create_test_engine();
        assert!(!engine.has_time_pricing("claude-sonnet-4"));
    }

    #[test]
    fn test_size() {
        let (engine, _) = create_test_engine();
        assert_eq!(engine.size(), 2);
    }

    #[test]
    fn test_user_override_replaces_cloud() {
        let (mut engine, app_db) = create_test_engine();
        app_db.save_override("claude-sonnet-4", 30.0, 150.0, 3.0, 37.5).unwrap();
        engine.refresh(&app_db).unwrap();
        let p = engine.get_pricing("claude-sonnet-4").unwrap();
        assert!(p.is_override);
        assert!((p.input_cost_per_million - 30.0).abs() < 0.001);
    }
}
