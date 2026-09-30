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
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

/// 原生菜单里的"启用菜单栏显示"开关：切换运行状态并持久化（主线程调用，
/// 返回新状态供菜单项同步勾选）
#[cfg(target_os = "macos")]
fn toggle_menubar_display(app: &tauri::AppHandle) -> bool {
    let app_state = app.state::<AppState>();
    let handle = app.state::<Mutex<services::menubar::MenubarHandle>>();
    let mut h = handle.lock().unwrap();
    let enable = !h.is_running();
    {
        let app_db = app_state.app_db.lock().unwrap();
        let _ = app_db.set_setting("menubar_display_enabled", if enable { "1" } else { "0" });
    }
    if enable {
        if let Err(e) = h.start(app_state.shared.clone()) {
            log::error!("菜单栏显示启动失败: {}", e);
        }
    } else {
        h.stop();
    }
    h.is_running()
}

/// 触发更新检查：窗口不在（后台模式）时置待处理标志，等窗口唤起后由前端
/// `consume_pending_update` 拉取——`emit` 在 webview 缺席时无人接收。
fn request_check_update(app: &tauri::AppHandle) {
    use services::headless;
    if headless::is_headless_mode() {
        headless::set_pending_update();
        log::info!("[Headless] 后台模式，更新检查已排队，窗口唤起时补发");
    } else {
        let _ = app.emit("check-update", ());
    }
}

/// 菜单栏/托盘里的「后台模式」：**立即**切换（进入则销毁窗口 / 退出则重建），
/// 返回切换后是否处于后台模式。
///
/// 用 `try_state` 而非 `state`：菜单回调跑在主线程，panic 无法 unwind 会直接
/// abort 整个进程（取的类型一旦与 `setup` 里 `manage` 的不一致就是这个下场）。
fn toggle_background_mode(app: &tauri::AppHandle) -> bool {
    if app.try_state::<services::headless::HeadlessState>().is_none() {
        log::error!("后台模式状态未注册，无法切换");
        return false;
    }
    match services::headless::toggle_headless(app) {
        Ok(now_in_background) => now_in_background,
        Err(e) => {
            log::error!("[Headless] 切换后台模式失败: {e}");
            services::headless::is_headless_mode()
        }
    }
}

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
    let should_exit_window = should_exit.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 第二个实例启动时，激活已有窗口（后台模式下窗口已销毁 → 重建）
            services::headless::show_or_rebuild_window(app);
        }))
        // 窗口关闭拦截挂在 Builder 上而非具体窗口实例上：后台模式会销毁
        // 窗口，绑定实例的拦截器在重建后的新窗口上会失效
        .on_window_event(move |window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if should_exit_window.load(Ordering::SeqCst) {
                    // Cmd+Q / 系统关机等，允许关闭
                    return;
                }
                api.prevent_close();
                let app = window.app_handle();
                let destroy = app
                    .try_state::<services::headless::HeadlessState>()
                    .map(|h| h.destroy_on_close())
                    .unwrap_or(false);
                if destroy {
                    if let Err(e) = services::headless::enter_headless_mode(app) {
                        log::error!("进入后台模式失败: {}", e);
                        let _ = window.hide();
                    }
                } else {
                    let _ = window.hide();
                }
            }
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;

            // 后台模式状态：先于任何回调注册（菜单回调会读它）
            app.manage(services::headless::HeadlessState::load(app.handle()));

            // 系统托盘（Windows / Linux；macOS 使用原生 NSStatusItem，见下方分支）
            #[cfg(not(target_os = "macos"))]
            {
                let menu = tauri::menu::MenuBuilder::new(app)
                    .item(&tauri::menu::MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?)
                    .separator()
                    .item(&tauri::menu::MenuItem::with_id(app, "background-mode", "进入后台模式", true, None::<&str>)?)
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
                            services::headless::show_or_rebuild_window(tray.app_handle());
                        }
                    })
                    .on_menu_event(move |app_handle, event| {
                        if event.id() == "quit" {
                            app_handle.exit(0);
                        } else if event.id() == "show" {
                            services::headless::show_or_rebuild_window(app_handle);
                        } else if event.id() == "background-mode" {
                            // 动作项：点击立即进入/退出门面，顺带把标题改成反向文案
                            let in_background = toggle_background_mode(app_handle);
                            if let Some(item) = app_handle
                                .menu()
                                .and_then(|m| m.get("background-mode"))
                            {
                                let _ = item.set_text(if in_background {
                                    "退出门面（返回界面）"
                                } else {
                                    "进入后台模式"
                                });
                            }
                            log::info!("[Headless] 后台模式切换 → {}", in_background);
                        } else if event.id() == "check-update" {
                            request_check_update(app_handle);
                        }
                    })
                    .build(app)?;
            }

            // macOS 原生菜单栏（NSStatusItem：图标 + 两行小字 + 原生菜单）
            #[cfg(target_os = "macos")]
            {
                use services::menubar_macos::{self, MenuCallbacks};
                let app_handle = app.handle().clone();
                let show_handle = app_handle.clone();
                let toggle_handle = app_handle.clone();
                let background_handle = app_handle.clone();
                let update_handle = app_handle.clone();
                let quit_handle = app_handle.clone();
                let callbacks = MenuCallbacks {
                    show_window: Box::new(move || {
                        services::headless::show_or_rebuild_window(&show_handle);
                    }),
                    toggle_enabled: Box::new(move || toggle_menubar_display(&toggle_handle)),
                    toggle_background: Box::new(move || toggle_background_mode(&background_handle)),
                    check_update: Box::new(move || {
                        request_check_update(&update_handle);
                    }),
                    quit: Box::new(move || quit_handle.exit(0)),
                };
                menubar_macos::init(
                    objc2::MainThreadMarker::new().expect("菜单栏初始化必须在主线程"),
                    callbacks,
                )?;
            }

            // 菜单栏显示服务 + 按持久化设置恢复（仅 macOS 生效）
            app.manage(Mutex::new(services::menubar::MenubarHandle::new(
                app.handle().clone(),
            )));

            #[cfg(target_os = "macos")]
            {
                let app_state = app.state::<AppState>();
                let app_db = app_state.app_db.lock().unwrap();
                let enabled = app_db.get_setting("menubar_display_enabled").as_deref() == Some("1");
                drop(app_db);
                if enabled {
                    let handle = app.state::<Mutex<services::menubar::MenubarHandle>>();
                    let mut h = handle.lock().unwrap();
                    if let Err(e) = h.start(app_state.shared.clone()) {
                        log::error!("恢复菜单栏显示失败: {}", e);
                    }
                    let _ = app.run_on_main_thread(|| {
                        services::menubar_macos::set_toggle_state(true);
                    });
                }

                // 「后台模式」动作项按当前实际状态初始化（AppKit 要求主线程）
                let _ = app.run_on_main_thread(|| {
                    services::menubar_macos::set_background_state(
                        services::headless::is_headless_mode(),
                    );
                });
            }

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
            commands::database::get_ccs_session_filter,
            commands::database::set_ccs_session_filter,
            commands::database::scan_dsh_now,
            commands::database::scan_minimax_now,
            commands::database::scan_kimi_now,
            commands::database::scan_pi_now,
            commands::database::scan_antigravity_now,
            commands::database::scan_proma_now,
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
            // 菜单栏显示（macOS）
            commands::menubar::get_menubar_status,
            commands::menubar::toggle_menubar_display,
            // 后台常驻模式
            commands::headless::get_background_mode,
            commands::headless::set_background_mode,
            commands::headless::toggle_headless_mode,
            commands::headless::consume_pending_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // 监听系统级退出请求（macOS Cmd+Q 等）
    // ExitRequested 在 CloseRequested 之前触发，设置标记让窗口允许关闭
    // Reopen: 点击 Dock 图标时重新显示窗口
    app.run(move |app_handle, event| {
        match event {
            RunEvent::ExitRequested { api, code, .. } => {
                // Tauri v2 在「所有窗口被销毁」时会自动退出进程（官方 issue
                // #13511 至今 open）。后台模式下窗口正是被我们主动销毁的，
                // 此时必须阻止退出，否则托盘/菜单栏会随之消失。
                // code 为 None 即代表这个「被动」来源；code 为 Some(_) 是用户
                // 主动 app.exit(0)（托盘菜单"退出" / Cmd+Q），照常退出。
                if code.is_none() && services::headless::is_headless_mode() {
                    log::info!("[Headless] 无存活窗口的退出请求，阻止退出并保持后台常驻");
                    api.prevent_exit();
                    return;
                }
                should_exit.store(true, Ordering::SeqCst);
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => {
                services::headless::show_or_rebuild_window(app_handle);
            }
            _ => {}
        }
        // 非 macOS 时 app_handle 未被使用，抑制 unused 变量警告
        #[cfg(not(target_os = "macos"))]
        let _ = &app_handle;
    });
}
