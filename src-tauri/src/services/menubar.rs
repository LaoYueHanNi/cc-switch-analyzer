//! macOS 菜单栏常驻显示：在托盘图标旁展示今日 Token 总量与费用。
//!
//! 渲染由 `menubar_macos`（原生 NSStatusItem，两行 9pt 小字）承担；
//! 本模块负责查询调度：后台线程每 30s 调共享查询管道，把结果
//! dispatch 到主线程更新。查询复用 `today_query`，持有独立的
//! `TodaySourceCache`，与 HTTP 服务 / 前端查询互不干扰。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::services::today_query::{query_today_data, TodaySourceCache};
use crate::SharedState;

/// 菜单栏刷新间隔（秒）。查询走独立 DataSource 长驻缓存，开销可忽略。
const MENUBAR_REFRESH_SECS: u64 = 30;

/// 菜单栏显示服务的管理句柄
pub struct MenubarHandle {
    app: tauri::AppHandle,
    shutdown: Option<Arc<AtomicBool>>,
    running: bool,
    /// 长驻独立数据源缓存：跨启停复用，避免每次刷新重建 DataSource
    source_cache: Arc<Mutex<TodaySourceCache>>,
}

impl MenubarHandle {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            shutdown: None,
            running: false,
            source_cache: Arc::new(Mutex::new(TodaySourceCache::default())),
        }
    }

    /// 启动菜单栏轮询线程（幂等：已在运行则直接返回）
    pub fn start(&mut self, shared: Arc<SharedState>) -> Result<(), String> {
        if self.running {
            return Ok(());
        }

        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = shutdown.clone();
        let thread_source_cache = self.source_cache.clone();
        let thread_app = self.app.clone();

        std::thread::Builder::new()
            .name("menubar-refresh".into())
            .spawn(move || {
                run_loop(thread_app, shared, thread_shutdown, thread_source_cache);
            })
            .map_err(|e| format!("启动菜单栏刷新线程失败: {}", e))?;

        self.shutdown = Some(shutdown);
        self.running = true;
        log::info!("[Menubar] 菜单栏显示已启动");
        Ok(())
    }

    /// 停止轮询并清空菜单栏文字（图标与菜单保留）
    pub fn stop(&mut self) {
        if !self.running {
            return;
        }
        if let Some(shutdown) = &self.shutdown {
            shutdown.store(true, Ordering::SeqCst);
        }
        apply_title(&self.app, String::new(), String::new());
        self.running = false;
        log::info!("[Menubar] 菜单栏显示已停止");
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

fn run_loop(
    app: tauri::AppHandle,
    shared: Arc<SharedState>,
    shutdown: Arc<AtomicBool>,
    source_cache: Arc<Mutex<TodaySourceCache>>,
) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let tz = (chrono::Local::now().offset().local_minus_utc() / 3600) as i64;
        // 查询失败 / 无数据源时显示占位符 "—"，保留图标存在感
        let (tokens, cost) = match query_today_data(&shared, tz, &source_cache) {
            Ok(data) => (
                format_tokens_compact(data.total_tokens),
                format_cost(data.total_cost_value),
            ),
            Err(_) => ("—".to_string(), String::new()),
        };
        apply_title(&app, tokens, cost);

        // 分段睡眠以便及时响应停止请求
        for _ in 0..(MENUBAR_REFRESH_SECS * 2) {
            if shutdown.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    log::info!("[Menubar] 刷新线程已退出");
}

/// 主线程更新菜单栏文字（AppKit 要求；空 token 表示清空）
#[cfg(target_os = "macos")]
fn apply_title(app: &tauri::AppHandle, tokens: String, cost: String) {
    let _ = app.run_on_main_thread(move || {
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            crate::services::menubar_macos::set_text(mtm, tokens, cost);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn apply_title(_app: &tauri::AppHandle, _tokens: String, _cost: String) {}

/// Token 数量缩写：<1k 原样；>=1k 用 k；>=1M 用 M。
/// 固定两位小数，使上下两行的数字位数稳定、可右对齐（见 `menubar_macos`）
fn format_tokens_compact(n: i64) -> String {
    let f = n as f64;
    if n >= 1_000_000 {
        format!("{:.2}M", f / 1e6)
    } else if n >= 1_000 {
        format!("{:.2}k", f / 1e3)
    } else {
        // 整数格式化会忽略精度（`{0:.2}` 仍是 "0"），需先转 f64
        format!("{:.2}", n as f64)
    }
}

/// 费用格式：符号后置（`12.34¥`），与 Token 的「数值 + 单位」排版一致
fn format_cost(v: f64) -> String {
    format!("{v:.2}¥")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tokens_compact() {
        assert_eq!(format_tokens_compact(0), "0.00");
        assert_eq!(format_tokens_compact(999), "999.00");
        assert_eq!(format_tokens_compact(1_500), "1.50k");
        // 四舍五入进位：9.999k -> 10.00k
        assert_eq!(format_tokens_compact(9_999), "10.00k");
        assert_eq!(format_tokens_compact(12_345), "12.35k");
        assert_eq!(format_tokens_compact(345_600), "345.60k");
        assert_eq!(format_tokens_compact(1_234_567), "1.23M");
    }

    #[test]
    fn test_format_cost() {
        assert_eq!(format_cost(0.0), "0.00¥");
        assert_eq!(format_cost(12.3), "12.30¥");
        assert_eq!(format_cost(33.986), "33.99¥");
        assert_eq!(format_cost(1725.5), "1725.50¥");
    }
}
