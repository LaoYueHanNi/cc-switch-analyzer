use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

use crate::models::FilterParams;
use crate::commands::query::compute_precompute;
use crate::services::data_source::create_source_entry;
use crate::SharedState;
use crate::utils::*;

/// TrafficMonitor 插件 HTTP 服务的管理句柄
pub struct TrafficMonitorServerHandle {
    shutdown: Arc<AtomicBool>,
    port: u16,
    running: AtomicBool,
}

impl TrafficMonitorServerHandle {
    pub fn new() -> Self {
        Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            port: 0,
            running: AtomicBool::new(false),
        }
    }

    /// 启动 TrafficMonitor API 服务（专用线程 + 同步 TCP）
    pub fn start(&mut self, shared: Arc<SharedState>) -> Result<u16, String> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(self.port);
        }

        let (port, listener) = bind_port()?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let cache: Arc<Mutex<TmCache>> = Arc::new(Mutex::new(TmCache::default()));

        let thread_shutdown = shutdown.clone();
        let thread_cache = cache.clone();
        let thread_shared = shared;

        std::thread::Builder::new()
            .name("tm-api-server".into())
            .spawn(move || {
                run_server(listener, thread_cache, thread_shutdown, thread_shared);
            })
            .map_err(|e| format!("启动服务线程失败: {}", e))?;

        self.shutdown = shutdown;
        self.port = port;
        self.running.store(true, Ordering::SeqCst);
        log::info!("[TM] TrafficMonitor API 服务已启动，端口 {}", port);
        Ok(port)
    }

    /// 停止服务（连接自身唤醒阻塞的 accept，使线程能检查 shutdown 标志并退出）
    pub fn stop(&mut self) {
        if !self.running.load(Ordering::SeqCst) {
            return;
        }
        self.shutdown.store(true, Ordering::SeqCst);
        if self.port > 0 {
            let _ = std::net::TcpStream::connect(format!("127.0.0.1:{}", self.port));
        }
        self.running.store(false, Ordering::SeqCst);
        log::info!("[TM] TrafficMonitor API 服务已停止");
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

// ========== 缓存 ==========

struct TmCache {
    data: Option<TmTodayData>,
    tz: Option<i64>,
    updated_at: Option<Instant>,
}

impl Default for TmCache {
    fn default() -> Self {
        Self {
            data: None,
            tz: None,
            updated_at: None,
        }
    }
}

#[derive(Clone, Serialize)]
struct TmTodayData {
    #[serde(rename = "totalTokens")]
    total_tokens: i64,
    #[serde(rename = "inputTokens")]
    input_tokens: i64,
    #[serde(rename = "outputTokens")]
    output_tokens: i64,
    #[serde(rename = "cacheReadTokens")]
    cache_read_tokens: i64,
    #[serde(rename = "cacheCreationTokens")]
    cache_creation_tokens: i64,
    #[serde(rename = "totalCost")]
    total_cost: String,
    #[serde(rename = "requestCount")]
    request_count: i64,
}

// ========== 同步 HTTP 服务器 ==========

fn run_server(
    listener: std::net::TcpListener,
    cache: Arc<Mutex<TmCache>>,
    shutdown: Arc<AtomicBool>,
    shared: Arc<SharedState>,
) {
    log::info!("[TM] API 服务线程已启动");

    listener.set_nonblocking(false).ok();

    for stream in listener.incoming() {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        let stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };

        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();

        let cache = cache.clone();
        let shared = shared.clone();
        handle_request(stream, cache, shared);
    }

    log::info!("[TM] API 服务线程已退出");
}

fn handle_request(
    mut stream: std::net::TcpStream,
    cache: Arc<Mutex<TmCache>>,
    shared: Arc<SharedState>,
) {
    use std::io::{Read, Write};

    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let path_with_query = parts[1];

    let (status, body) = if method != "GET" {
        ("405 Method Not Allowed".to_string(), r#"{"error":"method_not_allowed"}"#.to_string())
    } else if path_with_query.starts_with("/api/ping") {
        ("200 OK".to_string(), r#"{"status":"ok"}"#.to_string())
    } else if path_with_query.starts_with("/api/today") {
        let tz = parse_tz_param(path_with_query);
        handle_today(cache, shared, tz)
    } else {
        ("404 Not Found".to_string(), r#"{"error":"not_found"}"#.to_string())
    };

    // NOTE: `\` 续行符跳过换行及前导空白，缩进不会出现在实际 HTTP 响应中
    let response = format!(
        "HTTP/1.1 {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        status,
        body.len(),
        body
    );

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn handle_today(cache: Arc<Mutex<TmCache>>, shared: Arc<SharedState>, tz: i64) -> (String, String) {
    {
        let c = cache.lock().unwrap();
        if let (Some(data), Some(cached_tz), Some(updated)) = (&c.data, c.tz, c.updated_at) {
            if cached_tz == tz && updated.elapsed().as_secs() < TM_CACHE_TTL_SECS {
                return ("200 OK".to_string(), serde_json::to_string(data).unwrap());
            }
        }
    }

    match query_today_data(&shared, tz) {
        Ok(data) => {
            let mut c = cache.lock().unwrap();
            c.data = Some(data.clone());
            c.tz = Some(tz);
            c.updated_at = Some(Instant::now());
            ("200 OK".to_string(), serde_json::to_string(&data).unwrap())
        }
        Err(e) => (
            "500 Internal Server Error".to_string(),
            serde_json::json!({"error": e}).to_string(),
        ),
    }
}

fn parse_tz_param(path: &str) -> i64 {
    path.split('?')
        .nth(1)
        .unwrap_or("")
        .split('&')
        .find(|p| p.starts_with("tz="))
        .and_then(|p| p[3..].parse().ok())
        .unwrap_or(8)
}

// ========== 复用前端查询管道 ==========

/// 通过共享的 compute_precompute 管道查询今日数据。
/// 使用独立的 DataSource 实例，避免与前端的 rusqlite Connection 并发冲突。
fn query_today_data(shared: &SharedState, tz_offset: i64) -> Result<TmTodayData, String> {
    let now = now_epoch_seconds();
    let local_now = now + tz_offset * 3600;
    let today_start_local = local_now - (local_now % 86400);
    let from_epoch = today_start_local - tz_offset * 3600;
    let to_epoch = from_epoch + 86400;

    let params = FilterParams {
        from_epoch: Some(from_epoch),
        to_epoch: Some(to_epoch),
        tz_offset: Some(tz_offset),
        provider_id: None,
        model_id: None,
    };

    // 只在锁期间读取路径列表，释放后创建独立的 DataSource 实例
    // 按 enabled 过滤：HTTP 接口需尊重用户在 UI 里的数据源开关，否则
    // create_source_entry 重建时会硬编码 enabled=true，被禁用的源会"复活"
    let paths: Vec<String> = {
        let sources = shared.data_sources.read().map_err(|e| e.to_string())?;
        sources.iter().filter(|s| s.enabled).map(|s| s.path.clone()).collect()
    };
    if paths.is_empty() {
        return Err("no_database_loaded".to_string());
    }

    // 为每个路径创建独立的只读 DataSource（独立 SQLite 连接）
    let independent_sources: Vec<_> = paths
        .iter()
        .filter_map(|p| create_source_entry(p).ok())
        .collect();

    if independent_sources.is_empty() {
        return Err("no_database_loaded".to_string());
    }

    let pricing = shared.pricing_engine.read().map_err(|e| e.to_string())?;
    let result = compute_precompute(&independent_sources, &pricing, &params)?;
    let summary = result.summary;

    let total_cost: f64 = result.precomputed.model_costs.values().sum();

    Ok(TmTodayData {
        total_tokens: summary.total_input + summary.total_output + summary.total_cache_read + summary.total_cache_creation,
        input_tokens: summary.total_input,
        output_tokens: summary.total_output,
        cache_read_tokens: summary.total_cache_read,
        cache_creation_tokens: summary.total_cache_creation,
        total_cost: format!("{:.2}¥", (total_cost * 100.0).round() / 100.0),
        request_count: summary.total_requests,
    })
}

// ========== 端口绑定 ==========

fn bind_port() -> Result<(u16, std::net::TcpListener), String> {
    for port in TM_API_PORT..=TM_API_PORT_MAX {
        match std::net::TcpListener::bind(format!("127.0.0.1:{}", port)) {
            Ok(listener) => return Ok((port, listener)),
            Err(_) => continue,
        }
    }
    Err(format!(
        "无法绑定端口 {}-{}，所有端口均被占用",
        TM_API_PORT, TM_API_PORT_MAX
    ))
}
