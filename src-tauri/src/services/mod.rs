pub mod app_db;
pub mod ai_proxy_db;
pub mod cloud_pricing;
pub mod codex_sessions;
pub mod cursor_csv;
pub mod cursor_attribution;
pub mod cursor_hook_backup;
pub mod cursor_local_hook;
pub mod cursor_sync;
pub mod data_source;
pub mod dedup;
pub mod external_db;
#[cfg(target_os = "windows")]
pub mod http_server;
pub mod multi_terminal;
pub mod opencode_db;
pub mod pipeline;
pub mod precompute;
pub mod pricing_engine;
pub mod session_title;

/// 非 Windows 平台的空实现
#[cfg(not(target_os = "windows"))]
pub mod http_server {
    use std::sync::atomic::{AtomicBool, AtomicU16};
    use std::sync::Arc;

    pub struct TrafficMonitorServerHandle {
        _shutdown: Arc<AtomicBool>,
        _port: AtomicU16,
        _running: AtomicBool,
    }

    impl TrafficMonitorServerHandle {
        pub fn new() -> Self {
            Self {
                _shutdown: Arc::new(AtomicBool::new(false)),
                _port: AtomicU16::new(0),
                _running: AtomicBool::new(false),
            }
        }

        pub fn start(&mut self, _shared: Arc<crate::SharedState>) -> Result<u16, String> {
            Err("TrafficMonitor 插件仅支持 Windows".to_string())
        }

        pub fn stop(&mut self) {}

        pub fn is_running(&self) -> bool { false }

        pub fn port(&self) -> u16 { 0 }
    }
}
