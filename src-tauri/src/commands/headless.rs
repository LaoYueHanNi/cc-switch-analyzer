use tauri::State;

use crate::services::headless::{self, HeadlessState, HeadlessStatus};

/// 查询后台模式状态（`active` 为当前是否已销毁窗口，`destroyOnClose` 为配置）
#[tauri::command]
pub fn get_background_mode(state: State<'_, HeadlessState>) -> HeadlessStatus {
    HeadlessStatus {
        active: headless::is_headless_mode(),
        destroy_on_close: state.destroy_on_close(),
    }
}

/// 设置"关闭按钮是否销毁窗口"
///
/// **注意这是配置而非动作**：只影响之后点关闭按钮的行为，不会销毁当前窗口。
/// 想立刻进入后台模式用 `toggle_headless_mode`，或点菜单栏/托盘的「进入后台模式」。
#[tauri::command]
pub fn set_background_mode(
    app: tauri::AppHandle,
    enabled: bool,
    state: State<'_, HeadlessState>,
) -> Result<HeadlessStatus, String> {
    state.set_destroy_on_close(&app, enabled)?;
    Ok(HeadlessStatus {
        active: headless::is_headless_mode(),
        destroy_on_close: state.destroy_on_close(),
    })
}

/// **立即**切换后台模式：当前在后台则重建窗口返回界面，否则销毁窗口进入后台。
/// 返回切换后是否处于后台模式。
#[tauri::command]
pub fn toggle_headless_mode(app: tauri::AppHandle) -> Result<bool, String> {
    headless::toggle_headless(&app)
}

/// 取出后台期间排队的"检查更新"请求。
///
/// 后台模式下 webview 已被销毁，`emit` 无人接收；前端在挂载并注册完监听后
/// 调用本命令补拉一次，避免"点了没反应"。
#[tauri::command]
pub fn consume_pending_update() -> bool {
    headless::take_pending_update()
}
