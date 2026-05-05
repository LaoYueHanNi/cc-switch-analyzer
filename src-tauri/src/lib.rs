mod commands;
mod models;
mod services;
mod utils;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use services::app_db::AppDbService;
use services::external_db::ExternalDbService;
use services::pricing_engine::PricingEngine;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};
use tauri::{Manager, RunEvent, WindowEvent};

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

    // macOS Cmd+Q 先触发 ExitRequested 再触发 CloseRequested
    // 用此标记区分"系统退出"和"用户点关闭按钮"
    let should_exit = Arc::new(AtomicBool::new(false));
    let should_exit_close = should_exit.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // 系统托盘
            let menu = tauri::menu::MenuBuilder::new(app)
                .item(&tauri::menu::MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?)
                .separator()
                .item(&tauri::menu::MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?)
                .build()?;
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .tooltip("CC-Switch Analyzer")
                .menu(&menu)
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let win = tray.app_handle().get_webview_window("main").unwrap();
                        win.show().unwrap();
                        win.set_focus().unwrap();
                    }
                })
                .on_menu_event(move |app_handle, event| {
                    if event.id() == "quit" {
                        app_handle.exit(0);
                    } else if event.id() == "show" {
                        let win = app_handle.get_webview_window("main").unwrap();
                        win.show().unwrap();
                        win.set_focus().unwrap();
                    }
                })
                .build(app)?;

            // 点击关闭按钮时隐藏到托盘（系统退出请求除外）
            let win = app.get_webview_window("main").unwrap();
            let win_clone = win.clone();
            win.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    if should_exit_close.load(Ordering::SeqCst) {
                        // Cmd+Q / 系统关机等，允许关闭
                    } else {
                        // 用户点击关闭按钮，隐藏到托盘
                        api.prevent_close();
                        let _ = win_clone.hide();
                    }
                }
            });

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
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // 监听系统级退出请求（macOS Cmd+Q 等）
    // ExitRequested 在 CloseRequested 之前触发，设置标记让窗口允许关闭
    // Reopen: 点击 Dock 图标时重新显示窗口
    app.run(move |app_handle, event| {
        match event {
            RunEvent::ExitRequested { .. } => {
                should_exit.store(true, Ordering::SeqCst);
            }
            RunEvent::Reopen { .. } => {
                if let Some(win) = app_handle.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            _ => {}
        }
    });
}
