use tauri::State;

use crate::AppState;
use crate::models::*;

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
                user_aliases: user_alias_map.get(&p.model_id).cloned().unwrap_or_default(),
                no_cache_support: pricing.get_no_cache_support(&p.model_id),
            }
        })
        .collect())
}

#[tauri::command]
pub fn get_pricing_overrides(state: State<AppState>) -> Result<Vec<PricingOverride>, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.get_all_overrides()
}

#[tauri::command]
pub fn set_pricing_override(
    model_id: String,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    state: State<AppState>,
) -> Result<(), String> {
    if model_id.trim().is_empty() {
        return Err("模型 ID 不能为空".to_string());
    }
    if input < 0.0 || output < 0.0 || cache_read < 0.0 || cache_creation < 0.0 {
        return Err("价格不能为负数".to_string());
    }
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.save_override(&model_id, input, output, cache_read, cache_creation)
}

#[tauri::command]
pub fn remove_pricing_override(model_id: String, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.delete_override(&model_id)
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
    state: State<AppState>,
) -> Result<i64, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.add_time_override(&model_id, start_time, end_time, input, output, cache_read, cache_creation, &label)
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
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.update_time_override(id, start_time, end_time, input, output, cache_read, cache_creation, &label)
}

#[tauri::command]
pub fn delete_time_pricing_rule(
    id: i64,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.delete_time_override(id)
}

#[tauri::command]
pub fn save_override_context_tier(
    model_id: String,
    threshold: i64,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.save_override_tier(&model_id, threshold, input, output, cache_read, cache_creation)
}

#[tauri::command]
pub fn delete_override_context_tier(
    model_id: String,
    threshold: i64,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.delete_override_tier(&model_id, threshold)
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
    state: State<AppState>,
) -> Result<i64, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.add_time_override_tier(&model_id, start_time, end_time, threshold, input, output, cache_read, cache_creation)
}

#[tauri::command]
pub fn update_time_rule_context_tier(
    id: i64,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.update_time_override_tier(id, input, output, cache_read, cache_creation)
}

#[tauri::command]
pub fn delete_time_rule_context_tier(
    id: i64,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.delete_time_override(id)
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
    app_db.add_user_alias(&model_id, &alias)?;
    pricing.refresh(&app_db)
}

#[tauri::command]
pub fn remove_user_alias(model_id: String, alias: String, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    app_db.remove_user_alias(&model_id, &alias)?;
    pricing.refresh(&app_db)
}
