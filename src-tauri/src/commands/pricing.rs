use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::services::app_db::AppDbService;
use crate::services::pricing_engine::PricingEngine;

#[tauri::command]
pub fn get_all_pricing(state: State<AppState>) -> Result<Vec<PricingData>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    log::debug!("[PRICING] get_all_pricing: 引擎模型数={}", pricing.size());

    let user_alias_map = app_db.get_user_aliases().unwrap_or_default();

    let mut used_models: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in sources.iter() {
        if let Ok(models) = entry.source.get_models() {
            used_models.extend(models);
        }
    }

    let mut resolved_used_model_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for m in &used_models {
        if let Some(resolved) = pricing.resolve_model_id(m) {
            resolved_used_model_ids.insert(resolved);
        }
    }

    let all = pricing.get_all_pricing();
    log::debug!("[PRICING] 返回定价数据: {} 条", all.len());
    Ok(all
        .iter()
        .map(|p| {
            PricingData {
                model_id: p.model_id.clone(),
                input_cost_per_million: p.input_cost_per_million,
                output_cost_per_million: p.output_cost_per_million,
                cache_read_cost_per_million: p.cache_read_cost_per_million,
                cache_creation_cost_per_million: p.cache_creation_cost_per_million,
                is_override: p.is_override,
                has_time_pricing: pricing.has_time_pricing(&p.model_id),
                time_rules: pricing.get_time_rules(&p.model_id),
                is_used: resolved_used_model_ids.contains(&p.model_id),
                context_tiers: pricing.get_override_tiers(&p.model_id),
                cloud_time_rules: pricing.get_cloud_time_rules(&p.model_id),
                aliases: pricing.get_aliases(&p.model_id),
                alias_patterns: pricing.get_alias_patterns(&p.model_id),
                user_aliases: user_alias_map.get(&p.model_id).cloned().unwrap_or_default(),
                no_cache_support: pricing.get_no_cache_support(&p.model_id),
                family: pricing.get_family(&p.model_id),
                daily_slots: pricing.get_model_daily_slots(&p.model_id),
            }
        })
        .collect())
}

#[tauri::command]
pub fn get_pricing_families(state: State<AppState>) -> Result<Vec<PricingFamily>, String> {
    let pricing = state.pricing_engine.read().map_err(|e| e.to_string())?;
    Ok(pricing.get_families().to_vec())
}

#[tauri::command]
pub fn get_pricing_overrides(state: State<AppState>) -> Result<Vec<PricingOverride>, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.get_all_overrides()
}

fn apply_set_pricing_override(
    app_db: &AppDbService,
    pricing: &mut PricingEngine,
    model_id: &str,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
) -> Result<(), String> {
    let model_id = pricing.model_id_for_write(model_id)?;
    app_db.save_override(&model_id, input, output, cache_read, cache_creation)?;
    pricing.refresh(app_db)
}

#[tauri::command]
pub fn set_pricing_override(
    model_id: String,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    daily_slots: Option<Vec<DailySlot>>,
    state: State<AppState>,
) -> Result<(), String> {
    if model_id.trim().is_empty() {
        return Err("模型 ID 不能为空".to_string());
    }
    if input < 0.0 || output < 0.0 || cache_read < 0.0 || cache_creation < 0.0 {
        return Err("价格不能为负数".to_string());
    }
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    if let Some(slots) = daily_slots {
        let model_id = pricing.model_id_for_write(&model_id)?;
        app_db.save_override_with_slots(&model_id, input, output, cache_read, cache_creation, &slots)?;
        pricing.refresh(&app_db)
    } else {
        apply_set_pricing_override(&app_db, &mut pricing, &model_id, input, output, cache_read, cache_creation)
    }
}

fn apply_remove_pricing_override(
    app_db: &AppDbService,
    pricing: &mut PricingEngine,
    model_id: &str,
) -> Result<(), String> {
    let model_id = pricing.model_id_for_write(model_id)?;
    app_db.delete_override(&model_id)?;
    pricing.refresh(app_db)
}

#[tauri::command]
pub fn remove_pricing_override(model_id: String, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    apply_remove_pricing_override(&app_db, &mut pricing, &model_id)
}

#[tauri::command]
pub fn get_time_pricing_rules(state: State<AppState>) -> Result<Vec<TimePricingRule>, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.get_all_time_overrides()
}

#[tauri::command]
pub fn add_time_pricing_rule(
    model_id: String,
    start_time: i64,
    end_time: i64,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    label: String,
    daily_slots: Option<Vec<DailySlot>>,
    state: State<AppState>,
) -> Result<i64, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    let slots = daily_slots.unwrap_or_default();
    let id = app_db.add_time_override_with_slots(
        &pricing.model_id_for_write(&model_id)?, start_time, end_time, input, output, cache_read, cache_creation, &label, &slots,
    )?;
    pricing.refresh(&app_db)?;
    Ok(id)
}

#[tauri::command]
pub fn update_time_pricing_rule(
    id: i64,
    start_time: i64,
    end_time: i64,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    label: String,
    daily_slots: Option<Vec<DailySlot>>,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    app_db.update_time_override_with_slots(
        id, start_time, end_time, input, output, cache_read, cache_creation, &label, daily_slots.as_deref(),
    )?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn delete_time_pricing_rule(
    id: i64,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    app_db.delete_time_override(id)?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn save_override_context_tier(
    model_id: String,
    threshold: i64,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    daily_slots: Option<Vec<DailySlot>>,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    let slots = daily_slots.unwrap_or_default();
    app_db.save_override_tier_with_slots(&pricing.model_id_for_write(&model_id)?, threshold, input, output, cache_read, cache_creation, &slots)?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn delete_override_context_tier(
    model_id: String,
    threshold: i64,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    app_db.delete_override_tier(&pricing.model_id_for_write(&model_id)?, threshold)?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn save_time_rule_context_tier(
    model_id: String,
    start_time: i64,
    end_time: i64,
    threshold: i64,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    daily_slots: Option<Vec<DailySlot>>,
    state: State<AppState>,
) -> Result<i64, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    let slots = daily_slots.unwrap_or_default();
    let id = app_db.add_time_override_tier_with_slots(
        &pricing.model_id_for_write(&model_id)?, start_time, end_time, threshold, input, output, cache_read, cache_creation, &slots,
    )?;
    pricing.refresh(&app_db)?;
    Ok(id)
}

#[tauri::command]
pub fn update_time_rule_context_tier(
    id: i64,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    daily_slots: Option<Vec<DailySlot>>,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    app_db.update_time_override_tier_with_slots(
        id, input, output, cache_read, cache_creation, daily_slots.as_deref(),
    )?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn delete_time_rule_context_tier(
    id: i64,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    app_db.delete_time_override(id)?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn refresh_pricing(state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn fetch_cloud_pricing(state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    pricing.fetch_and_cache_cloud_pricing(&app_db)?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn add_user_alias(model_id: String, alias: String, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    app_db.add_user_alias(&pricing.model_id_for_write(&model_id)?, &alias)?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn remove_user_alias(model_id: String, alias: String, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    app_db.remove_user_alias(&pricing.model_id_for_write(&model_id)?, &alias)?;
    pricing.refresh(&app_db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cloud_pricing;

    const CANONICAL: &str = "gemini-3.8-flash";
    const HISTORICAL: &str = "gemini-3.8-flash-n";

    fn import_table(db: &AppDbService) {
        let table = cloud_pricing::parse_cloud_pricing(r#"{
            "version": 113, "updatedAt": 1, "currency": "RMB", "models": [{
                "modelId": "gemini-3.8-flash", "inputCostPerMillion": 2, "outputCostPerMillion": 6,
                "cacheReadCostPerMillion": 0.2, "cacheCreationCostPerMillion": 3,
                "aliasPatterns": ["^gemini-3\\.8-flash(-(low|medium|high|n))?$"]
            }]
        }"#).unwrap();
        db.save_cloud_pricing(&table).unwrap();
    }

    #[test]
    fn historical_owner_alias_price_edit_and_restore_use_canonical_key() {
        let db = AppDbService::new_in_memory().unwrap();
        db.save_override(HISTORICAL, 30.0, 60.0, 3.0, 40.0).unwrap();
        db.add_user_alias(HISTORICAL, "my-gemini").unwrap();
        // A historical self-alias must not mask owner normalization.
        db.add_user_alias(HISTORICAL, HISTORICAL).unwrap();
        let mut engine = PricingEngine::new();
        engine.refresh(&db).unwrap();
        assert_eq!(engine.resolve_model_id("my-gemini").as_deref(), Some(HISTORICAL));
        import_table(&db);
        engine.refresh(&db).unwrap();
        assert_eq!(engine.size(), 1);
        assert_eq!(engine.resolve_model_id("MY-GEMINI").as_deref(), Some(CANONICAL));
        assert_eq!(engine.get_pricing_at("my-gemini", 0, 0).unwrap().input_cost_per_million, 30.0);
        assert!(engine.get_aliases(CANONICAL).contains(&"my-gemini".to_string()));
        assert!(db.get_user_aliases().unwrap()[CANONICAL].contains(&"my-gemini".to_string()));
        assert_eq!(db.get_all_overrides().unwrap()[0].model_id, CANONICAL);

        apply_set_pricing_override(&db, &mut engine, CANONICAL, 50.0, 100.0, 5.0, 60.0).unwrap();
        apply_set_pricing_override(&db, &mut engine, "MY-GEMINI", 70.0, 140.0, 7.0, 80.0).unwrap();
        assert_eq!(db.get_all_overrides().unwrap().len(), 1);
        engine.refresh(&db).unwrap();
        assert_eq!(engine.get_pricing_at(HISTORICAL, 0, 0).unwrap().input_cost_per_million, 70.0);
        db.remove_user_alias(&engine.model_id_for_write(HISTORICAL).unwrap(), "my-gemini").unwrap();
        engine.refresh(&db).unwrap();
        assert!(engine.resolve_model_id("my-gemini").is_none());
        assert!(!db.get_user_aliases().unwrap()[CANONICAL].contains(&"my-gemini".to_string()));
        apply_remove_pricing_override(&db, &mut engine, CANONICAL).unwrap();
        assert!(db.get_all_overrides().unwrap().is_empty());
        assert!(!engine.get_pricing_at(HISTORICAL, 0, 0).unwrap().is_override);
        assert_eq!(engine.get_pricing_at(HISTORICAL, 0, 0).unwrap().input_cost_per_million, 2.0);
    }

    #[test]
    fn historical_tiers_slots_and_time_rule_ids_survive_migration_and_remain_editable() {
        let db = AppDbService::new_in_memory().unwrap();
        let slots = vec![DailySlot {
            label: "peak".into(), days_of_week: None,
            windows: vec![DailyWindow { start_minute: 0, end_minute: 60 }],
            input_cost_per_million: 90.0, output_cost_per_million: 180.0,
            cache_read_cost_per_million: 9.0, cache_creation_cost_per_million: 100.0,
        }];
        db.save_override_with_slots(HISTORICAL, 30.0, 60.0, 3.0, 40.0, &slots).unwrap();
        db.save_override_tier(HISTORICAL, 100, 40.0, 80.0, 4.0, 50.0).unwrap();
        let root_id = db.add_time_override(HISTORICAL, 100, 200, 50.0, 100.0, 5.0, 60.0, "local").unwrap();
        let tier_id = db.add_time_override_tier(HISTORICAL, 100, 200, 100, 60.0, 120.0, 6.0, 70.0).unwrap();
        import_table(&db);
        let mut engine = PricingEngine::new();
        engine.refresh(&db).unwrap();
        assert_eq!(engine.get_pricing_at(HISTORICAL, 0, 0).unwrap().input_cost_per_million, 90.0);
        assert_eq!(engine.get_pricing_at_with_context(HISTORICAL, 0, 100, 0).unwrap().input_cost_per_million, 40.0);
        let rules = db.get_all_time_overrides().unwrap();
        assert_eq!(rules[0].model_id, CANONICAL);
        assert_eq!(rules[0].id, root_id);
        assert_eq!(rules[0].context_tiers[0].id, Some(tier_id));
        db.add_time_override_tier(&engine.model_id_for_write(HISTORICAL).unwrap(), 100, 200, 200, 80.0, 160.0, 8.0, 90.0).unwrap();
        db.delete_override_tier(&engine.model_id_for_write("GEMINI-3.8-FLASH-N").unwrap(), 100).unwrap();
        engine.refresh(&db).unwrap();
        assert!(db.get_all_overrides().unwrap()[0].context_tiers.is_empty());
        assert_eq!(engine.get_pricing_at_with_context(HISTORICAL, 150, 200, 0).unwrap().input_cost_per_million, 80.0);
        apply_remove_pricing_override(&db, &mut engine, HISTORICAL).unwrap();
        assert!(db.get_all_overrides().unwrap().is_empty());
    }

    #[test]
    fn collisions_choose_newest_prices_and_keep_nonconflicting_tiers_and_aliases() {
        let db = AppDbService::new_in_memory().unwrap();
        db.save_override(CANONICAL, 20.0, 40.0, 2.0, 30.0).unwrap();
        db.save_override(HISTORICAL, 30.0, 60.0, 3.0, 40.0).unwrap();
        db.save_override_tier(CANONICAL, 100, 40.0, 80.0, 4.0, 50.0).unwrap();
        db.save_override_tier(HISTORICAL, 100, 50.0, 100.0, 5.0, 60.0).unwrap();
        db.save_override_tier(HISTORICAL, 200, 60.0, 120.0, 6.0, 70.0).unwrap();
        db.conn().execute("UPDATE pricing_overrides SET updated_at=10", []).unwrap();
        db.conn().execute("UPDATE pricing_overrides SET updated_at=20 WHERE model_id=? AND threshold=0", [HISTORICAL]).unwrap();
        db.add_user_alias(HISTORICAL, "my-gemini").unwrap();
        db.add_user_alias(CANONICAL, "my-gemini").unwrap();
        db.add_time_override(HISTORICAL, 100, 200, 70.0, 140.0, 7.0, 80.0, "old").unwrap();
        let newest_id = db.add_time_override(CANONICAL, 100, 200, 80.0, 160.0, 8.0, 90.0, "new").unwrap();
        import_table(&db);
        let mut engine = PricingEngine::new();
        engine.refresh(&db).unwrap();
        let overrides = db.get_all_overrides().unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].input_cost_per_million, 30.0);
        assert_eq!(overrides[0].context_tiers.len(), 2);
        assert_eq!(overrides[0].context_tiers[0].input_cost_per_million, 40.0);
        assert_eq!(db.get_user_aliases().unwrap()[CANONICAL], vec!["my-gemini"]);
        assert_eq!(db.get_all_time_overrides().unwrap().len(), 1);
        assert_eq!(db.get_all_time_overrides().unwrap()[0].id, newest_id);
        engine.refresh(&db).unwrap();
        assert_eq!(engine.get_pricing_at_with_context(HISTORICAL, 150, 0, 0).unwrap().input_cost_per_million, 80.0);
    }

    #[test]
    fn unknown_custom_models_keep_their_price_aliases_and_crud() {
        let db = AppDbService::new_in_memory().unwrap();
        db.save_override("custom-model", 30.0, 60.0, 3.0, 40.0).unwrap();
        db.add_user_alias("custom-model", "my-custom").unwrap();
        import_table(&db);
        let mut engine = PricingEngine::new();
        engine.refresh(&db).unwrap();
        assert_eq!(engine.size(), 2);
        assert_eq!(db.get_all_overrides().unwrap()[0].model_id, "custom-model");
        apply_set_pricing_override(&db, &mut engine, "MY-CUSTOM", 50.0, 100.0, 5.0, 60.0).unwrap();
        assert_eq!(engine.get_pricing_at("custom-model", 0, 0).unwrap().input_cost_per_million, 50.0);
        apply_remove_pricing_override(&db, &mut engine, "CUSTOM-MODEL").unwrap();
        assert!(db.get_all_overrides().unwrap().is_empty());
        assert!(engine.get_pricing_at("my-custom", 0, 0).is_none());
    }

    #[test]
    fn normalization_write_failure_rolls_back_all_keys_and_preserves_active_engine() {
        let db = AppDbService::new_in_memory().unwrap();
        db.save_override(HISTORICAL, 30.0, 60.0, 3.0, 40.0).unwrap();
        db.add_user_alias(HISTORICAL, "my-gemini").unwrap();
        db.add_time_override(HISTORICAL, 100, 200, 50.0, 100.0, 5.0, 60.0, "local").unwrap();
        let mut engine = PricingEngine::new();
        engine.refresh(&db).unwrap();
        import_table(&db);
        db.conn().execute_batch(
            "CREATE TRIGGER fail_alias_move BEFORE INSERT ON model_aliases
             WHEN NEW.model_id = 'gemini-3.8-flash'
             BEGIN SELECT RAISE(ABORT, 'test migration failure'); END;",
        ).unwrap();
        assert!(engine.refresh(&db).is_err());
        assert_eq!(db.get_all_overrides().unwrap()[0].model_id, HISTORICAL);
        assert_eq!(db.get_all_time_overrides().unwrap()[0].model_id, HISTORICAL);
        assert!(db.get_user_aliases().unwrap().contains_key(HISTORICAL));
        assert_eq!(engine.resolve_model_id("my-gemini").as_deref(), Some(HISTORICAL));
        db.conn().execute_batch("DROP TRIGGER fail_alias_move").unwrap();
        engine.refresh(&db).unwrap();
        assert_eq!(engine.resolve_model_id("my-gemini").as_deref(), Some(CANONICAL));
    }

    #[test]
    fn ambiguous_alias_cannot_save_or_remove_pricing() {
        let db = AppDbService::new_in_memory().unwrap();
        import_table(&db);
        db.save_override("custom-model", 30.0, 60.0, 3.0, 40.0).unwrap();
        db.add_user_alias(CANONICAL, "shared-alias").unwrap();
        db.add_user_alias("custom-model", "SHARED-ALIAS").unwrap();
        let mut engine = PricingEngine::new();
        engine.refresh(&db).unwrap();
        assert!(apply_set_pricing_override(&db, &mut engine, "shared-alias", 50.0, 100.0, 5.0, 60.0).unwrap_err().contains("歧义"));
        assert!(apply_remove_pricing_override(&db, &mut engine, "shared-alias").unwrap_err().contains("歧义"));
        assert_eq!(db.get_all_overrides().unwrap().len(), 1);
        assert_eq!(engine.get_pricing_at("custom-model", 0, 0).unwrap().input_cost_per_million, 30.0);
    }
}
