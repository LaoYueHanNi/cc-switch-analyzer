pub mod database;
pub mod pricing;
pub mod query;
pub mod session_manager;
pub mod session_title;
pub mod task;
#[cfg(target_os = "windows")]
pub mod traffic_monitor;

/// 非 Windows 平台的空实现，避免 invoke_handler 中引用不到
#[cfg(not(target_os = "windows"))]
pub mod traffic_monitor {
    use serde::Serialize;
    use tauri::State;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct TmServiceStatus {
        pub enabled: bool,
        pub running: bool,
        pub port: u16,
    }

    #[tauri::command]
    pub fn get_http_service_status() -> TmServiceStatus {
        TmServiceStatus { enabled: false, running: false, port: 0 }
    }

    #[tauri::command]
    pub fn toggle_http_service(_enabled: bool) -> Result<TmServiceStatus, String> {
        Err("TrafficMonitor 插件仅支持 Windows".to_string())
    }

    #[tauri::command]
    pub fn download_traffic_monitor_plugin(_arch: String) -> Result<String, String> {
        Err("TrafficMonitor 插件仅支持 Windows".to_string())
    }
}
