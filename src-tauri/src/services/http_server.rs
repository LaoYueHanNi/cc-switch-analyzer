use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::services::today_query::{query_today_data, TodaySourceCache, TodayTimezone};
use crate::SharedState;

/// TrafficMonitor 插件 HTTP 服务的管理句柄
pub struct TrafficMonitorServerHandle {
    shutdown: Arc<AtomicBool>,
    port: u16,
    running: AtomicBool,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TrafficMonitorServerHandle {
    pub fn new() -> Self {
        Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            port: 0,
            running: AtomicBool::new(false),
            thread: None,
        }
    }

    /// 启动 TrafficMonitor API 服务（专用线程 + 同步 TCP）
    pub fn start(&mut self, shared: Arc<SharedState>) -> Result<u16, String> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(self.port);
        }

        let (port, listener) = bind_port()?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let source_cache: Arc<Mutex<TodaySourceCache>> =
            Arc::new(Mutex::new(TodaySourceCache::default()));

        let thread_shutdown = shutdown.clone();
        let thread_source_cache = source_cache.clone();
        let thread_shared = shared;

        let thread = std::thread::Builder::new()
            .name("tm-api-server".into())
            .spawn(move || {
                run_server(
                    listener,
                    thread_source_cache,
                    thread_shutdown,
                    thread_shared,
                );
            })
            .map_err(|e| format!("启动服务线程失败: {}", e))?;

        self.thread = Some(thread);
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
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
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

impl Drop for TrafficMonitorServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

// ========== 同步 HTTP 服务器 ==========

fn run_server(
    listener: std::net::TcpListener,
    source_cache: Arc<Mutex<TodaySourceCache>>,
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
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .ok();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(1)))
            .ok();

        let source_cache = source_cache.clone();
        let shared = shared.clone();
        handle_request(stream, source_cache, shared);
    }

    log::info!("[TM] API 服务线程已退出");
}

fn handle_request(
    mut stream: std::net::TcpStream,
    source_cache: Arc<Mutex<TodaySourceCache>>,
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
        (
            "405 Method Not Allowed".to_string(),
            r#"{"error":"method_not_allowed"}"#.to_string(),
        )
    } else if path_with_query.starts_with("/api/ping") {
        ("200 OK".to_string(), r#"{"status":"ok"}"#.to_string())
    } else if path_with_query.starts_with("/api/today") {
        match parse_tz_param(path_with_query) {
            Ok(tz) => handle_today(source_cache, shared, tz),
            Err(e) => (
                "400 Bad Request".into(),
                serde_json::json!({"error": e}).to_string(),
            ),
        }
    } else {
        (
            "404 Not Found".to_string(),
            r#"{"error":"not_found"}"#.to_string(),
        )
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

fn handle_today(
    source_cache: Arc<Mutex<TodaySourceCache>>,
    shared: Arc<SharedState>,
    tz: TodayTimezone,
) -> (String, String) {
    // Minimal aggregation is cheap enough to query on demand. No TTL can cross midnight
    // or hide source/filter/pricing changes, including external SQLite WAL commits.
    match query_today_data(&shared, tz, &source_cache) {
        Ok(data) => ("200 OK".into(), serde_json::to_string(&data).unwrap()),
        Err(e) => (
            "503 Service Unavailable".into(),
            serde_json::json!({"error": e}).to_string(),
        ),
    }
}

fn parse_tz_param(path: &str) -> Result<TodayTimezone, String> {
    let params: Vec<_> = path
        .split_once('?')
        .map(|(_, q)| q.split('&').collect())
        .unwrap_or_default();
    if params.contains(&"tz=local") {
        return Ok(TodayTimezone::Local);
    }
    if let Some(p) = params.iter().find_map(|p| p.strip_prefix("tz_minutes=")) {
        let minutes: i32 = p.parse().map_err(|_| "invalid_timezone")?;
        if !(-1439..=1439).contains(&minutes) {
            return Err("invalid_timezone".into());
        }
        return Ok(TodayTimezone::OffsetMinutes(minutes));
    }
    // Backward compatibility for existing DLLs: tz remains an integer hour offset.
    if let Some(p) = params.iter().find_map(|p| p.strip_prefix("tz=")) {
        let hours: i32 = p.parse().map_err(|_| "invalid_timezone")?;
        if !(-23..=23).contains(&hours) {
            return Err("invalid_timezone".into());
        }
        return Ok(TodayTimezone::OffsetMinutes(hours * 60));
    }
    Ok(TodayTimezone::Local)
}

// ========== 端口绑定 ==========

fn bind_port() -> Result<(u16, std::net::TcpListener), String> {
    use crate::utils::{TM_API_PORT, TM_API_PORT_MAX};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::today_query::tests::{fixture, insert};

    #[test]
    fn response_sees_wal_and_source_toggle_without_ttl() {
        let (_dir, db, shared, cache) = fixture();
        let now = crate::utils::now_epoch_seconds();
        insert(&db, "PI", "first", now, 100);
        let (_, body) = handle_today(cache.clone(), shared.clone(), TodayTimezone::Local);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["totalTokens"],
            160
        );
        insert(&db, "PI", "second", now, 600);
        let (_, body) = handle_today(cache.clone(), shared.clone(), TodayTimezone::Local);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["totalTokens"],
            820
        );
        shared.data_sources.write().unwrap()[0].enabled = false;
        assert_eq!(
            handle_today(cache, shared, TodayTimezone::Local).0,
            "503 Service Unavailable"
        );
    }

    #[test]
    fn legacy_hours_and_fractional_offsets_are_validated() {
        assert!(matches!(
            parse_tz_param("/api/today?tz=8"),
            Ok(TodayTimezone::OffsetMinutes(480))
        ));
        assert!(matches!(
            parse_tz_param("/api/today?tz_minutes=330"),
            Ok(TodayTimezone::OffsetMinutes(330))
        ));
        assert!(matches!(
            parse_tz_param("/api/today?tz=local"),
            Ok(TodayTimezone::Local)
        ));
        assert!(parse_tz_param("/api/today?tz_minutes=-2147483648").is_err());
        assert!(parse_tz_param("/api/today?tz=99999999999999999").is_err());
    }

    #[test]
    fn stop_joins_listener_and_allows_immediate_restart_on_same_port() {
        let (_dir, _db, shared, _cache) = fixture();
        let mut server = TrafficMonitorServerHandle::new();
        let port = server.start(shared.clone()).unwrap();
        server.stop();
        let probe = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        drop(probe);
        assert_eq!(server.start(shared).unwrap(), port);
        server.stop();
    }
}
