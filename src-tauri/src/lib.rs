mod commands;
mod models;
mod services;
mod utils;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;
use services::app_db::AppDbService;
use services::data_source::SourceEntry;
use services::dedup::RequestCache;
use services::pricing_engine::PricingEngine;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

// 跨线程共享的状态（Tauri 命令 + HTTP 服务共用）
pub struct SharedState {
    pub data_sources: RwLock<Vec<SourceEntry>>,
    pub pricing_engine: RwLock<PricingEngine>,
}

impl SharedState {
    pub fn new(pricing_engine: PricingEngine) -> Self {
        Self {
            data_sources: RwLock::new(Vec::new()),
            pricing_engine: RwLock::new(pricing_engine),
        }
    }
}

// 全局应用状态
pub struct AppState {
    shared: Arc<SharedState>,
    app_db: Mutex<AppDbService>,
    db_latest_timestamp: Mutex<Option<i64>>,
    db_file_mtimes: Mutex<HashMap<String, std::time::SystemTime>>,
    request_cache: Mutex<RequestCache>,
}

// 通过 Deref，Tauri 命令中 state.data_sources 仍然直接可用
impl std::ops::Deref for AppState {
    type Target = SharedState;
    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_db = AppDbService::new().expect("初始化应用数据库失败");
    let pricing_engine = PricingEngine::new();

    let shared = Arc::new(SharedState::new(pricing_engine));

    let state = AppState {
        shared: shared.clone(),
        app_db: Mutex::new(app_db),
        db_latest_timestamp: Mutex::new(None),
        db_file_mtimes: Mutex::new(HashMap::new()),
        request_cache: Mutex::new(RequestCache::new(5000)),
    };

    // macOS Cmd+Q 先触发 ExitRequested 再触发 CloseRequested
    // 用此标记区分"系统退出"和"用户点关闭按钮"
    let should_exit = Arc::new(AtomicBool::new(false));
    let should_exit_close = should_exit.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 第二个实例启动时，激活已有窗口
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;

            // 系统托盘
            let menu = tauri::menu::MenuBuilder::new(app)
                .item(&tauri::menu::MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?)
                .separator()
                .item(&tauri::menu::MenuItem::with_id(app, "check-update", "检查更新", true, None::<&str>)?)
                .item(&tauri::menu::MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?)
                .build()?;
            let tray_builder = if let Some(icon) = app.default_window_icon() {
                TrayIconBuilder::new().icon(icon.clone())
            } else {
                TrayIconBuilder::new()
            };
            let _tray = tray_builder
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
                        if let Some(win) = tray.app_handle().get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .on_menu_event(move |app_handle, event| {
                    if event.id() == "quit" {
                        app_handle.exit(0);
                    } else if event.id() == "show" {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    } else if event.id() == "check-update" {
                        let _ = app_handle.emit("check-update", ());
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

            // 恢复 HTTP 服务状态（如果上次启用，仅 Windows）
            #[cfg(target_os = "windows")]
            {
                let app_state = app.state::<AppState>();
                let app_db = app_state.app_db.lock().unwrap();
                let enabled = app_db.get_setting("tm_service_enabled").as_deref() == Some("1");
                drop(app_db);
                if enabled {
                    let handle = app.state::<Mutex<services::http_server::TrafficMonitorServerHandle>>();
                    let mut h = handle.lock().unwrap();
                    if let Err(e) = h.start(app_state.shared.clone()) {
                        log::error!("恢复 HTTP 服务失败: {}", e);
                    } else {
                        log::info!("HTTP 服务已恢复，端口 {}", h.port());
                    }
                }
            }

            Ok(())
        })
        .manage(state)
        .manage(Mutex::new(services::http_server::TrafficMonitorServerHandle::new()))
        .invoke_handler(tauri::generate_handler![
            // 数据库操作
            commands::database::auto_load_database,
            commands::database::load_database,
            commands::database::add_database,
            commands::database::remove_database,
            commands::database::toggle_database,
            commands::database::list_databases,
            commands::database::refresh_database,
            commands::database::get_filter_options,
            commands::database::get_default_paths,
            commands::database::get_ccs_auto_discover,
            commands::database::set_ccs_auto_discover,
            commands::database::scan_dsh_now,
            commands::database::scan_minimax_now,
            commands::database::dsh_settings,
            commands::database::set_dsh_plugin_mode,
            commands::database::set_dsh_plugin_data_dir,
            commands::database::open_plugin_repo,
            // Cursor 数据源
            commands::cursor::cursor_login,
            commands::cursor::cursor_sync,
            commands::cursor::cursor_status,
            commands::cursor::cursor_preview_csv,
            commands::cursor::cursor_set_attribution_override,
            commands::cursor::cursor_clear_attribution_override,
            commands::cursor::cursor_toggle_attribution,
            commands::cursor::cursor_set_hook_writing,
            commands::cursor::cursor_set_attribution_filter_start,
            commands::cursor::cursor_set_sync_lookback,
            commands::cursor::cursor_set_hook_backup_period,
            commands::cursor::cursor_backup_hooks_now,
            commands::cursor::cursor_merge_hooks_now,
            commands::cursor::cursor_logout,
            // 数据查询
            commands::query::query_summary,
            commands::query::query_by_model,
            commands::query::query_by_provider,
            commands::query::query_provider_model_tokens,
            commands::query::query_daily_trend,
            commands::query::query_hourly_trend,
            commands::query::query_sessions,
            commands::query::query_session_model_tokens,
            commands::query::query_session_request_tokens,
            commands::query::query_session_timestamps,
            commands::query::query_realtime,
            commands::query::query_realtime_logs,
            commands::query::query_precompute,
            commands::query::query_sessions_with_cost,
            commands::query::query_session_project_groups,
            commands::query::query_project_session_details,
            // 会话标题
            commands::session_title::get_session_titles,
            // 会话管理
            commands::session_manager::open_claude_terminal,
            commands::session_manager::open_opencode_terminal,
            commands::session_manager::resume_claude_session,
            commands::session_manager::delete_claude_session,
            commands::session_manager::resume_opencode_session,
            commands::session_manager::get_ccswitch_providers,
            commands::session_manager::open_claude_terminal_with_provider,
            commands::session_manager::resume_claude_session_with_provider,
            commands::session_manager::open_codex_terminal,
            commands::session_manager::resume_codex_session,
            commands::session_manager::open_grok_terminal,
            commands::session_manager::resume_grok_session,
            // 定价操作
            commands::pricing::get_all_pricing,
            commands::pricing::get_pricing_families,
            commands::pricing::get_pricing_overrides,
            commands::pricing::set_pricing_override,
            commands::pricing::remove_pricing_override,
            commands::pricing::get_time_pricing_rules,
            commands::pricing::add_time_pricing_rule,
            commands::pricing::update_time_pricing_rule,
            commands::pricing::delete_time_pricing_rule,
            commands::pricing::refresh_pricing,
            commands::pricing::fetch_cloud_pricing,
            // 上下文定价档位
            commands::pricing::save_override_context_tier,
            commands::pricing::delete_override_context_tier,
            commands::pricing::save_time_rule_context_tier,
            commands::pricing::update_time_rule_context_tier,
            commands::pricing::delete_time_rule_context_tier,
            // 用户别名
            commands::pricing::add_user_alias,
            commands::pricing::remove_user_alias,
            // 任务管理
            commands::task::list_tasks,
            commands::task::get_task_detail,
            commands::task::create_task,
            commands::task::update_task,
            commands::task::delete_task,
            commands::task::add_sessions_to_task,
            commands::task::get_task_session_detail,
            commands::task::open_task_agent,
            commands::task::open_task_sessions,
            // TrafficMonitor 插件服务
            commands::traffic_monitor::get_http_service_status,
            commands::traffic_monitor::toggle_http_service,
            commands::traffic_monitor::download_traffic_monitor_plugin,
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
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => {
                if let Some(win) = app_handle.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            _ => {}
        }
        // 非 macOS 时 app_handle 未被使用，抑制 unused 变量警告
        #[cfg(not(target_os = "macos"))]
        let _ = &app_handle;
    });
}
