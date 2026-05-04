use tauri::State;

use crate::AppState;
use crate::models::*;

#[tauri::command]
pub fn get_exchange_rate(state: State<AppState>) -> Result<f64, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    Ok(app_db.get_exchange_rate())
}

#[tauri::command]
pub fn set_exchange_rate(rate: f64, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.set_exchange_rate(rate)
}

#[tauri::command]
pub fn get_all_pricing(state: State<AppState>) -> Result<Vec<PricingData>, String> {
    let pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    eprintln!("[PRICING] get_all_pricing: 引擎模型数={}", pricing.size());

    let mut used_models: std::collections::HashSet<String> = std::collections::HashSet::new();
    if ext_db.is_open() {
        if let Ok(models) = ext_db.get_models() {
            used_models = models.into_iter().collect();
        }
    }

    let all = pricing.get_all_pricing();
    eprintln!("[PRICING] 返回定价数据: {} 条", all.len());
    Ok(all
        .iter()
        .map(|p| {
            let time_rules = pricing.get_time_rules(&p.model_id);
            PricingData {
                model_id: p.model_id.clone(),
                display_name: p.display_name.clone(),
                input_cost_per_million: p.input_cost_per_million,
                output_cost_per_million: p.output_cost_per_million,
                cache_read_cost_per_million: p.cache_read_cost_per_million,
                cache_creation_cost_per_million: p.cache_creation_cost_per_million,
                is_override: p.is_override,
                has_time_pricing: pricing.has_time_pricing(&p.model_id),
                time_rules,
                is_used: used_models.contains(&p.model_id),
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
pub fn delete_time_pricing_rule(id: i64, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.delete_time_override(id)
}

#[tauri::command]
pub fn refresh_pricing(state: State<AppState>) -> Result<(), String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;
    pricing.refresh(&ext_db, &app_db)
}
