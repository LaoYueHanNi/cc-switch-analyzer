//! Rust owns local collection, including while every WebView is destroyed.
//! Cursor API synchronization deliberately remains in foreground/manual commands.
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

use crate::services::{app_db::AppDbService, data_source::DbType};
use crate::AppState;

pub const COLLECTION_INTERVAL: Duration = Duration::from_secs(5);

pub struct BackgroundCollector {
    stop_tx: Mutex<mpsc::Sender<()>>,
}

impl BackgroundCollector {
    pub fn start(app: tauri::AppHandle) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("local-usage-collector".into())
            .spawn(move || {
                run_loop(rx, COLLECTION_INTERVAL, || {
                    collect_local_sources(&app.state::<AppState>())
                });
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stop_tx: Mutex::new(tx),
        })
    }

    pub fn stop(&self) {
        if let Ok(tx) = self.stop_tx.lock() {
            let _ = tx.send(());
        }
    }
}

impl Drop for BackgroundCollector {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_loop(
    rx: mpsc::Receiver<()>,
    interval: Duration,
    mut collect: impl FnMut() -> Result<(), String>,
) {
    loop {
        match rx.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if let Err(e) = collect() {
            log::warn!("[Collector] 本地采集失败: {e}");
        }
        match rx.recv_timeout(interval) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => break,
        }
    }
}

/// A single gate prevents foreground timers and the collector from duplicating scans.
pub(crate) fn collect_local_sources(state: &AppState) -> Result<(), String> {
    collect_with(state, COLLECTION_INTERVAL, scan_source)
}

fn collect_with(
    state: &AppState,
    minimum_interval: Duration,
    mut scan: impl FnMut(&AppDbService, &DbType) -> Result<(), String>,
) -> Result<(), String> {
    let mut last = state.collection_gate.lock().map_err(|e| e.to_string())?;
    if last.is_some_and(|t| t.elapsed() < minimum_interval) {
        return Ok(());
    }
    let types: Vec<DbType> = state
        .data_sources
        .read()
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|s| s.enabled)
        .map(|s| s.db_type.clone())
        .collect();
    let mut errors = Vec::new();
    for db_type in types {
        // A dedicated application DB connection keeps long scans off the UI settings mutex.
        let db = state.collection_db.lock().map_err(|e| e.to_string())?;
        if let Err(e) = scan(&db, &db_type) {
            log::warn!("[Collector] {} 采集失败，下一轮重试: {e}", db_type.label());
            errors.push(format!("{}: {e}", db_type.label()));
        }
    }
    *state.collection_errors.write().map_err(|e| e.to_string())? = errors;
    *last = Some(Instant::now());
    Ok(())
}

fn scan_source(db: &AppDbService, db_type: &DbType) -> Result<(), String> {
    use crate::services::*;
    let result = match db_type {
        DbType::Dsh => dsh_scanner::scan_dsh_by_mode(db)?,
        DbType::Minimax => minimax_scanner::scan_minimax(db)?,
        DbType::Kimi => kimi_scanner::scan_kimi(db)?,
        DbType::Pi => pi_scanner::scan_pi(db)?,
        DbType::Magpie => magpie_scanner::scan_magpie(db)?,
        DbType::Proma => proma_scanner::scan_proma(db)?,
        DbType::ZCode => zcode_scanner::scan_zcode(db)?,
        DbType::Antigravity => antigravity_scanner::scan_antigravity_with_backoff(db)?,
        // No Cursor API requests here.
        _ => return Ok(()),
    };
    if result.errors > 0 {
        return Err(format!("{} 个文件扫描失败", result.errors));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        dedup::RequestCache,
        today_query::{query_today_data, tests::fixture, TodayTimezone},
    };
    use std::sync::Arc;

    #[test]
    fn worker_imports_appended_session_without_webview_and_honors_disabled_sources() {
        let (dir, db, shared, cache) = fixture();
        let roots = vec![dir.path().join("pi-sessions")];
        std::fs::create_dir_all(&roots[0]).unwrap();
        let state = Arc::new(AppState {
            shared: shared.clone(),
            app_db: Mutex::new(db),
            collection_db: Mutex::new(
                AppDbService::new_test_file(&dir.path().join("pricing.db")).unwrap(),
            ),
            db_latest_timestamp: Mutex::new(None),
            db_file_mtimes: Mutex::new(Default::default()),
            request_cache: Mutex::new(RequestCache::new(5000)),
            sources_initialized: Mutex::new(true),
            collection_gate: Mutex::new(None),
        });
        let (stop_tx, stop_rx) = mpsc::channel();
        let (tick_tx, tick_rx) = mpsc::channel();
        let thread_state = state.clone();
        let worker = std::thread::spawn(move || {
            run_loop(stop_rx, Duration::from_millis(20), || {
                collect_with(&thread_state, Duration::from_millis(10), |db, kind| {
                    if *kind == DbType::Pi {
                        crate::services::pi_scanner::scan_pi_roots(db, &roots)?;
                    }
                    Ok(())
                })?;
                let _ = tick_tx.send(());
                Ok(())
            })
        });
        tick_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(
            query_today_data(&shared, TodayTimezone::Local, &cache)
                .unwrap()
                .total_tokens,
            0
        );
        shared.data_sources.write().unwrap()[0].enabled = false;
        let row = serde_json::json!({ "type":"message", "id":"background-new", "message":{
            "role":"assistant", "model":"test-model", "timestamp":crate::utils::now_epoch_seconds()*1000,
            "usage":{"input":600,"output":60,"cacheRead":0,"cacheWrite":0}
        }});
        std::fs::write(
            dir.path().join("pi-sessions/new.jsonl"),
            format!(
                "{{\"type\":\"session\",\"id\":\"background-session\",\"cwd\":\"/test\"}}\n{row}\n"
            ),
        )
        .unwrap();
        for _ in 0..3 {
            tick_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        assert_eq!(
            state
                .app_db
                .lock()
                .unwrap()
                .get_session_log_count("PI")
                .unwrap(),
            0
        );
        // Collection must proceed while the UI settings connection is occupied.
        let ui_guard = state.app_db.lock().unwrap();
        shared.data_sources.write().unwrap()[0].enabled = true;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            tick_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            if query_today_data(&shared, TodayTimezone::Local, &cache)
                .unwrap()
                .total_tokens
                == 660
            {
                break;
            }
            assert!(Instant::now() < deadline);
        }
        stop_tx.send(()).unwrap();
        worker.join().unwrap();
        // A stopped worker no longer invokes the collection closure.
        assert_eq!(ui_guard.get_session_log_count("PI").unwrap(), 1);
    }
}
