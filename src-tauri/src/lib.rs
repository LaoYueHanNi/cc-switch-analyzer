mod commands;
mod models;
mod services;
mod utils;

use std::sync::Mutex;
use services::app_db::AppDbService;
use services::external_db::ExternalDbService;
use services::pricing_engine::PricingEngine;

// 全局应用状态
pub struct AppState {
    external_db: Mutex<ExternalDbService>,
    app_db: Mutex<AppDbService>,
    pricing_engine: Mutex<PricingEngine>,
    db_latest_timestamp: Mutex<Option<i64>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_db = AppDbService::new().expect("初始化应用数据库失败");
    let external_db = ExternalDbService::new();
    let pricing_engine = PricingEngine::new();

    let state = AppState {
        external_db: Mutex::new(external_db),
        app_db: Mutex::new(app_db),
        pricing_engine: Mutex::new(pricing_engine),
        db_latest_timestamp: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            // 数据库操作
            commands::database::select_database,
            commands::database::auto_load_database,
            commands::database::load_database,
            commands::database::refresh_database,
            commands::database::get_filter_options,
            // 数据查询
            commands::query::query_summary,
            commands::query::query_by_model,
            commands::query::query_by_provider,
            commands::query::query_provider_model_tokens,
            commands::query::query_daily_trend,
            commands::query::query_cache_durations,
            commands::query::query_cache_windows,
            commands::query::query_sessions,
            commands::query::query_session_model_tokens,
            commands::query::query_session_request_tokens,
            commands::query::query_session_timestamps,
            commands::query::query_realtime,
            commands::query::query_realtime_logs,
            commands::query::query_precompute,
            commands::query::query_sessions_with_cost,
            // 会话标题
            commands::session_title::get_session_titles,
            // 定价操作
            commands::pricing::get_exchange_rate,
            commands::pricing::set_exchange_rate,
            commands::pricing::get_all_pricing,
            commands::pricing::get_pricing_overrides,
            commands::pricing::set_pricing_override,
            commands::pricing::remove_pricing_override,
            commands::pricing::get_time_pricing_rules,
            commands::pricing::add_time_pricing_rule,
            commands::pricing::update_time_pricing_rule,
            commands::pricing::delete_time_pricing_rule,
            commands::pricing::refresh_pricing,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
