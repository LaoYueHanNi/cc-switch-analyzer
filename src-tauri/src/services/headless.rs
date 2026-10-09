//! 后台常驻模式（headless）：销毁 WebView 窗口，只保留 Rust 主进程。
//!
//! 现状是关闭窗口走 `prevent_close + hide()`——窗口不可见但 WebView 进程完整
//! 存活，前端仍在跑自动刷新与实时轮询。本模块提供真正的「销毁」路径：窗口销毁
//! 后 WebView 子进程随之退出，托盘/菜单栏（`menubar.rs`）与 TrafficMonitor HTTP
//! 服务（`http_server.rs`）因本就是独立线程 + 独立 `TodaySourceCache`，不受影响。
//!
//! **Tauri v2 的坑**：所有窗口被销毁时，Tauri 会自动触发 `RunEvent::ExitRequested`
//! 并退出进程（官方 issue #13511 至今 open，无一等公民解法）。因此进入后台模式
//! 必须在 `lib.rs` 的 `ExitRequested` 分支中 `prevent_exit()`，本模块只负责窗口
//! 生命周期本身。
//!
//! 参考实现：cc-switch `src-tauri/src/lightweight.rs`（同为 Tauri v2，思路一致，
//! 但托盘结构不同——它用 Tauri tray 刷新菜单，本项目 macOS 走原生 NSStatusItem）。
//!
//! 决策记录：`docs/decisions/implemented/2026-09-30-headless-background-mode.md`

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindowBuilder};

use crate::AppState;

/// 主窗口 label，与 `tauri.conf.json` 的 `app.windows[0].label` 一致
const MAIN_LABEL: &str = "main";

/// 设置键：点关闭按钮时销毁窗口（true）还是仅隐藏（false）。
/// 默认 false —— 销毁能省内存，但冷启动 0.5–2s 且丢失路由/筛选状态，
/// 不应强加给存量用户。
const SETTING_DESTROY_ON_CLOSE: &str = "close_destroys_window";

/// 设置键：窗口几何（位置 + 大小 + 最大化态），JSON 编码的 [`WindowGeometry`]
const SETTING_GEOMETRY: &str = "window_geometry";

/// 是否处于后台模式（窗口已销毁，主进程存活）
const VISIBLE: u8 = 0;
const ENTERING: u8 = 1;
const BACKGROUND: u8 = 2;
const EXITING: u8 = 3;
static PHASE: AtomicU8 = AtomicU8::new(VISIBLE);
static REOPEN_PENDING: AtomicBool = AtomicBool::new(false);

/// 后台期间触发的「检查更新」待处理标志：webview 不存在时 `emit` 无人接收，
/// 置位后由 [`show_or_rebuild_window`] 在唤起时补发。
static PENDING_UPDATE: AtomicBool = AtomicBool::new(false);

/// 窗口几何。窗口销毁后重建会回落到配置默认值（1280×800），这里显式持久化。
/// 不用 `tauri-plugin-window-state` 是为了零新增依赖、且不依赖其窗口创建
/// hook 的时机——`WebviewWindowBuilder` 动态建窗与启动期建窗的路径不同。
#[derive(Serialize, Deserialize, Default, Clone, Copy)]
struct WindowGeometry {
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    #[serde(default)]
    maximized: bool,
    #[serde(default)]
    inner_size: bool,
}

impl WindowGeometry {
    /// 位置是否仍落在某块屏幕的可见区域内：拔掉外接显示器后旧坐标会指向
    /// 不存在的区域，直接套用会把窗口放到屏幕外无法拖回，故丢弃退回居中。
    fn is_position_visible(&self, app: &AppHandle) -> bool {
        let Some(window) = app.get_webview_window(MAIN_LABEL) else {
            return false;
        };
        // 拿不到 monitor 列表时保守沿用（宁可位置奇怪，也不要放屏外）
        let Ok(monitors) = window.available_monitors() else {
            return true;
        };
        if monitors.is_empty() {
            return true;
        }
        // 标题栏高度留 40px 容差：work_area 不含标题栏，贴着顶边是合法的
        const TOP_TOLERANCE: i64 = 40;
        monitors.iter().any(|m| {
            // position 是 i32、size 是 u32，统一提升到 i64 再比较
            let r = m.work_area();
            let (left, top) = (r.position.x as i64, r.position.y as i64);
            let (right, bottom) = (left + r.size.width as i64, top + r.size.height as i64);
            (self.x as i64) + (self.w as i64) > left
                && (self.x as i64) < right
                && (self.y as i64) + TOP_TOLERANCE > top
                && (self.y as i64) < bottom
        })
    }
}

/// 读取一个 settings 值。
///
/// 统一走 `try_state` + 锁的 `unwrap_or_else`：本模块的调用点包含**主线程**
/// （菜单栏回调、窗口关闭事件），那里 panic 无法 unwind 会直接 abort 整个
/// 进程（开发期实测：`state()` 类型不匹配即 `SIGABRT`），因此这里宁可降级
/// 也不能炸。
fn read_setting(app: &AppHandle, key: &str) -> Option<String> {
    let state = app.try_state::<AppState>()?;
    let app_db = state.app_db.lock().unwrap_or_else(|e| e.into_inner());
    app_db.get_setting(key)
}

/// 写入一个 settings 值，失败时返回错误（不 panic）
fn write_setting(app: &AppHandle, key: &str, value: &str) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or("应用状态未就绪，无法保存设置")?;
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.set_setting(key, value)
}

/// 后台模式状态（供前端展示用）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessStatus {
    /// 当前是否处于后台模式（窗口已销毁）
    pub active: bool,
    /// 关闭按钮是否配置为销毁窗口
    pub destroy_on_close: bool,
}

/// 「关闭按钮是否销毁窗口」的可变状态。
///
/// 由 `AppHandle::manage` 托管，使 Tauri 命令、托盘菜单、macOS 原生菜单回调
/// 三条路径共享同一份内存态；`destroy_on_close` 在本模块内被高频读取（每次窗口
/// 关闭事件），走内存而非每次查 SQLite。
pub struct HeadlessState {
    destroy_on_close: AtomicBool,
    normal_geometry: std::sync::Mutex<Option<WindowGeometry>>,
}

impl HeadlessState {
    /// 启动时从 `pricing.db::settings` 恢复；未设置时默认 `false`（仅隐藏）
    pub fn load(app: &AppHandle) -> Self {
        let destroy_on_close = read_setting(app, SETTING_DESTROY_ON_CLOSE)
            .map(|v| v == "1")
            .unwrap_or(false);
        log::info!(
            "[Headless] 启动加载：关闭按钮{}窗口",
            if destroy_on_close { "销毁" } else { "仅隐藏" }
        );
        Self {
            destroy_on_close: AtomicBool::new(destroy_on_close),
            normal_geometry: std::sync::Mutex::new(None),
        }
    }

    pub fn destroy_on_close(&self) -> bool {
        self.destroy_on_close.load(Ordering::Acquire)
    }

    /// 切换并持久化。落盘失败时保持内存态不变，调用方以本函数返回的最终态为准。
    pub fn set_destroy_on_close(&self, app: &AppHandle, value: bool) -> Result<(), String> {
        write_setting(app, SETTING_DESTROY_ON_CLOSE, if value { "1" } else { "0" })?;
        self.destroy_on_close.store(value, Ordering::Release);
        log::info!(
            "[Headless] 关闭按钮行为已切换为：{}",
            if value { "销毁窗口（后台模式）" } else { "仅隐藏窗口" }
        );
        Ok(())
    }
}

pub fn is_headless_mode() -> bool {
    PHASE.load(Ordering::Acquire) != VISIBLE
}

/// 后台模式下触发的更新检查请求，供窗口唤起时补发
pub fn set_pending_update() {
    PENDING_UPDATE.store(true, Ordering::Release);
}

/// 取出并清除待处理的更新检查请求
pub fn take_pending_update() -> bool {
    PENDING_UPDATE.swap(false, Ordering::AcqRel)
}

/// 平台相关外观：进入后台时把应用从 Dock / 任务栏"摘掉"，
/// 唤起时恢复。窗口在 macOS 上与 Dock 无关（后台模式窗口已销毁），
/// 在 Windows 上则要在 destroy 之前置 `skip_taskbar`。
fn apply_appearance(app: &AppHandle, background: bool) {
    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy;
        let policy = if background {
            ActivationPolicy::Accessory
        } else {
            ActivationPolicy::Regular
        };
        if let Err(e) = app.set_dock_visibility(!background) {
            log::warn!("[Headless] 设置 Dock 显示状态失败: {e}");
        }
        if let Err(e) = app.set_activation_policy(policy) {
            log::warn!("[Headless] 设置激活策略失败: {e}");
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(window) = app.get_webview_window(MAIN_LABEL) {
            if let Err(e) = window.set_skip_taskbar(background) {
                log::warn!("[Headless] 设置任务栏可见性失败: {e}");
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Linux 无统一的后台外观语义，窗口销毁后由托盘图标承担入口
        let _ = app;
    }
}

/// Resize/move events retain normal geometry so maximized windows restore correctly.
pub fn track_normal_geometry(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_LABEL) else { return };
    if window.is_maximized().unwrap_or(true) || window.is_minimized().unwrap_or(true) || window.is_fullscreen().unwrap_or(true) { return; }
    let (Ok(size), Ok(pos)) = (window.inner_size(), window.outer_position()) else { return };
    if size.width == 0 || size.height == 0 { return; }
    if let Some(state) = app.try_state::<HeadlessState>() {
        if let Ok(mut normal) = state.normal_geometry.lock() {
            *normal = Some(WindowGeometry { w: size.width, h: size.height, x: pos.x, y: pos.y, maximized: false, inner_size: true });
        }
    }
}

fn save_geometry(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_LABEL) else { return };
    let maximized = window.is_maximized().unwrap_or(false);
    if !maximized { track_normal_geometry(app); }
    let normal = app.try_state::<HeadlessState>().and_then(|s| s.normal_geometry.lock().ok().and_then(|g| *g));
    let previous = read_setting(app, SETTING_GEOMETRY).and_then(|v| serde_json::from_str::<WindowGeometry>(&v).ok()).filter(|g| g.inner_size);
    let Some(mut geometry) = normal.or(previous) else { return };
    geometry.maximized = maximized;
    if let Ok(encoded) = serde_json::to_string(&geometry) {
        if let Err(e) = write_setting(app, SETTING_GEOMETRY, &encoded) { log::warn!("[Headless] 保存几何失败: {e}"); }
    }
}

fn restore_geometry(app: &AppHandle) {
    let Some(raw) = read_setting(app, SETTING_GEOMETRY) else {
        return;
    };
    let Ok(geometry) = serde_json::from_str::<WindowGeometry>(&raw) else {
        return;
    };
    let Some(window) = app.get_webview_window(MAIN_LABEL) else {
        return;
    };
    // Old versions stored outer dimensions; never feed them into set_size (inner size).
    if !geometry.inner_size || geometry.w == 0 || geometry.h == 0 {
        return;
    }

    let _ = window.set_size(PhysicalSize::new(geometry.w, geometry.h));
    if geometry.is_position_visible(app) {
        let _ = window.set_position(PhysicalPosition::new(geometry.x, geometry.y));
    } else {
        log::info!("[Headless] 旧窗口位置已不在任何屏幕内，改为居中");
        let _ = window.center();
    }
    if geometry.maximized {
        let _ = window.maximize();
    }
}

/// 进入后台模式：保存窗口几何 → 调整平台外观 → 销毁窗口。
///
/// 窗口本来就不存在时只置标志位（幂等）。
/// Execute each transition on the event loop, including IPC and native callbacks.
fn on_main<T: Send + 'static>(app: &AppHandle, f: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || { let _ = tx.send(f(&handle)); }).map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

pub fn enter_headless_mode(app: &AppHandle) -> Result<(), String> {
    on_main(app, enter_on_main)
}

fn enter_on_main(app: &AppHandle) -> Result<(), String> {
    if PHASE.load(Ordering::Acquire) == ENTERING { REOPEN_PENDING.store(false, Ordering::Release); return Ok(()); }
    if PHASE.load(Ordering::Acquire) != VISIBLE { return Ok(()); }
    REOPEN_PENDING.store(false, Ordering::Release);
    save_geometry(app);
    // Set before asynchronous destroy, so the last-window exit request is prevented.
    PHASE.store(ENTERING, Ordering::Release);
    apply_appearance(app, true);
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        if let Err(e) = window.destroy() {
            PHASE.store(VISIBLE, Ordering::Release);
            apply_appearance(app, false);
            sync_tray_label(app);
            return Err(format!("销毁主窗口失败: {e}"));
        }
    } else {
        PHASE.store(BACKGROUND, Ordering::Release);
    }
    sync_tray_label(app);
    Ok(())
}

/// Called from RunEvent after Tauri removes the old window from its manager.
pub fn window_destroyed(app: &AppHandle) {
    if PHASE.compare_exchange(ENTERING, BACKGROUND, Ordering::AcqRel, Ordering::Acquire).is_err() { return; }
    log::info!("[Headless] 主窗口销毁已确认，后台服务继续运行");
    sync_tray_label(app);
    if REOPEN_PENDING.swap(false, Ordering::AcqRel) {
        // Enqueue from another thread, after the current native Destroyed event completes.
        let app = app.clone();
        std::thread::spawn(move || show_or_rebuild_window(&app));
    }
}

pub fn exit_headless_mode(app: &AppHandle) -> Result<(), String> {
    on_main(app, exit_on_main)
}

fn exit_on_main(app: &AppHandle) -> Result<(), String> {
    if PHASE.load(Ordering::Acquire) == ENTERING {
        REOPEN_PENDING.store(true, Ordering::Release);
        return Ok(());
    }
    let rebuilding = app.get_webview_window(MAIN_LABEL).is_none();
    PHASE.store(EXITING, Ordering::Release);
    let result = (|| {
        if rebuilding {
            let config = app.config().app.windows.iter().find(|w| w.label == MAIN_LABEL).ok_or("主窗口配置未找到")?.clone();
            WebviewWindowBuilder::from_config(app, &config).map_err(|e| e.to_string())?
                .visible(false).build().map_err(|e| format!("创建主窗口失败: {e}"))?;
            restore_geometry(app);
        }
        if let Some(window) = app.get_webview_window(MAIN_LABEL) {
            window.unminimize().map_err(|e| e.to_string())?;
            window.show().map_err(|e| e.to_string())?;
            apply_appearance(app, false);
            window.set_focus().map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    let exists = app.get_webview_window(MAIN_LABEL).is_some();
    PHASE.store(if exists { VISIBLE } else { BACKGROUND }, Ordering::Release);
    apply_appearance(app, !exists);
    sync_tray_label(app);
    result
}

/// 同步托盘/菜单栏「后台模式」项的标题与勾选态。
///
/// 进入/退出可能由任意线程触发（菜单回调在主线程、设置页命令在异步线程），
/// AppKit 状态项要求主线程，故统一走 `run_on_main_thread`。
fn sync_tray_label(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let in_background = is_headless_mode();
        let _ = app.run_on_main_thread(move || {
            crate::services::menubar_macos::set_background_state(in_background);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(item) = app.try_state::<BackgroundMenuItem>() {
            let _ = item.0.set_text(if is_headless_mode() { "返回界面" } else { "进入后台模式" });
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub struct BackgroundMenuItem(pub tauri::menu::MenuItem<tauri::Wry>);

/// 统一的窗口唤起入口：托盘点击 / 托盘菜单 / 菜单栏 / 重复启动 / 点 Dock 图标
/// 全部走这里，内部决定是"show 已有窗口"还是"重建窗口"。
pub fn show_or_rebuild_window(app: &AppHandle) {
    if let Err(e) = exit_headless_mode(app) {
        log::error!("[Headless] 唤起窗口失败: {e}");
    }
}

/// **立即**切换后台模式：已在后台则退出，否则进入。返回切换后的状态。
///
/// 与 `HeadlessState::set_destroy_on_close`（只改"下次关闭按钮的行为"）是两件
/// 不同的事：菜单栏/托盘这类"手边就有"的入口给动作而非配置，点了就见效；
/// 进入后台时顺带把 `destroy_on_close` 置为 true，否则关掉重建的窗口会立刻
/// 又被销毁，用户会以为"退不出后台模式"。
pub fn toggle_headless(app: &AppHandle) -> Result<bool, String> {
    on_main(app, |app| {
        if PHASE.load(Ordering::Acquire) == ENTERING {
            return Ok(REOPEN_PENDING.fetch_xor(true, Ordering::AcqRel));
        }
        if is_headless_mode() { exit_on_main(app)?; }
        else {
            if let Some(h) = app.try_state::<HeadlessState>() { h.set_destroy_on_close(app, true)?; }
            enter_on_main(app)?;
        }
        Ok(is_headless_mode())
    })
}
