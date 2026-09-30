use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::services::menubar::MenubarHandle;

/// 菜单栏显示服务状态
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MenubarStatus {
    pub supported: bool,
    pub enabled: bool,
    pub running: bool,
}

/// 查询菜单栏显示状态
#[tauri::command]
pub fn get_menubar_status(
    state: State<crate::AppState>,
    menubar: State<Mutex<MenubarHandle>>,
) -> MenubarStatus {
    let app_db = state.app_db.lock().unwrap();
    let enabled = app_db.get_setting("menubar_display_enabled").as_deref() == Some("1");
    drop(app_db);

    let handle = menubar.lock().unwrap();
    MenubarStatus {
        supported: cfg!(target_os = "macos"),
        enabled,
        running: handle.is_running(),
    }
}

/// 启停菜单栏显示（仅 macOS）
#[tauri::command]
pub fn toggle_menubar_display(
    app: tauri::AppHandle,
    enabled: bool,
    state: State<crate::AppState>,
    menubar: State<Mutex<MenubarHandle>>,
) -> Result<MenubarStatus, String> {
    if !cfg!(target_os = "macos") {
        return Err("菜单栏显示仅支持 macOS".to_string());
    }

    // 持久化设置
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        app_db.set_setting("menubar_display_enabled", if enabled { "1" } else { "0" })?;
    }

    let mut handle = menubar.lock().map_err(|e| e.to_string())?;
    if enabled {
        handle.start(state.shared.clone())?;
    } else {
        handle.stop();
    }

    // 同步原生菜单项勾选态（AppKit 要求主线程）
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(move || {
        crate::services::menubar_macos::set_toggle_state(enabled);
    });

    Ok(MenubarStatus {
        supported: true,
        enabled,
        running: handle.is_running(),
    })
}
