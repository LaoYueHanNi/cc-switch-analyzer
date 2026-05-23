use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::services::http_server::TrafficMonitorServerHandle;

/// TrafficMonitor 插件服务状态
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmServiceStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
}

/// 查询 TrafficMonitor 插件服务状态
#[tauri::command]
pub fn get_http_service_status(
    state: State<crate::AppState>,
    tm_server: State<Mutex<TrafficMonitorServerHandle>>,
) -> TmServiceStatus {
    let app_db = state.app_db.lock().unwrap();
    let enabled = app_db.get_setting("tm_service_enabled").as_deref() == Some("1");
    drop(app_db);

    let server = tm_server.lock().unwrap();
    TmServiceStatus {
        enabled,
        running: server.is_running(),
        port: if server.is_running() { server.port() } else { crate::utils::TM_API_PORT },
    }
}

/// 启停 TrafficMonitor 插件服务
#[tauri::command]
pub fn toggle_http_service(
    enabled: bool,
    state: State<crate::AppState>,
    tm_server: State<Mutex<TrafficMonitorServerHandle>>,
) -> Result<TmServiceStatus, String> {
    // 持久化设置
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        app_db.set_setting("tm_service_enabled", if enabled { "1" } else { "0" })?;
    }

    let mut server = tm_server.lock().map_err(|e| e.to_string())?;
    if enabled {
        server.start(state.shared.clone())?;
    } else {
        server.stop();
    }

    Ok(TmServiceStatus {
        enabled,
        running: server.is_running(),
        port: if server.is_running() { server.port() } else { crate::utils::TM_API_PORT },
    })
}

/// 下载 TrafficMonitor 插件 DLL 到用户 Downloads 文件夹（仅 Windows）
#[tauri::command]
#[cfg(target_os = "windows")]
pub fn download_traffic_monitor_plugin(arch: String) -> Result<String, String> {
    let dll_bytes: &[u8] = match arch.as_str() {
        "x86" => include_bytes!("../../resources/CCSwitchAnalyzer_x86.dll"),
        "x64" => include_bytes!("../../resources/CCSwitchAnalyzer_x64.dll"),
        _ => return Err(format!("不支持的架构: {}", arch)),
    };

    let downloads_dir = dirs::download_dir()
        .ok_or_else(|| "无法获取 Downloads 目录".to_string())?;

    let filename = format!("CCSwitchAnalyzer_{}.dll", arch);
    let target_path = downloads_dir.join(&filename);
    std::fs::write(&target_path, dll_bytes)
        .map_err(|e| format!("写入文件失败: {}", e))?;

    Ok(target_path.to_string_lossy().to_string())
}

#[tauri::command]
#[cfg(not(target_os = "windows"))]
pub fn download_traffic_monitor_plugin(_arch: String) -> Result<String, String> {
    Err("TrafficMonitor 插件仅支持 Windows".to_string())
}
