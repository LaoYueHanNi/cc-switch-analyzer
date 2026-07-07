use tauri::State;

use crate::AppState;
use crate::models::SourceInfo;
use crate::services::cursor_sync::{self, SyncCursorResult};
use crate::services::data_source::{create_source_entry, DbType};
use crate::utils;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorStatus {
    pub logged_in: bool,
    pub last_sync: Option<i64>,
    pub record_count: i64,
    pub cache_path: Option<String>,
    pub membership_type: Option<String>,
}

fn cursor_cache_dir_str() -> Result<String, String> {
    Ok(utils::get_cursor_cache_dir()?.to_string_lossy().to_string())
}

pub fn reload_cursor_sources(state: &State<AppState>) -> Result<(), String> {
    let cache_str = cursor_cache_dir_str()?;
    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    for entry in sources.iter_mut() {
        if matches!(entry.db_type, DbType::Cursor) {
            entry.source.open(&cache_str)?;
        }
    }
    Ok(())
}

fn ensure_cursor_source_registered(
    state: &State<AppState>,
) -> Result<(), String> {
    let cache_str = cursor_cache_dir_str()?;
    let csv_path = utils::get_cursor_usage_csv_path()?;
    if !csv_path.exists() {
        return Ok(());
    }

    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    if let Some(entry) = sources.iter_mut().find(|s| matches!(s.db_type, DbType::Cursor)) {
        entry.source.open(&cache_str)?;
        return Ok(());
    }

    match create_source_entry(&cache_str) {
        Ok(entry) => {
            log::info!("[CURSOR] 注册数据源: {}", cache_str);
            sources.push(entry);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn save_sources(state: &State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    let info: Vec<SourceInfo> = sources.iter().map(|s| s.to_info()).collect();
    crate::commands::database::save_paths_public(state, &info);
    Ok(info)
}

/// 查询前自动同步 Cursor 缓存并在有更新时重载数据源
pub fn sync_and_reload_if_needed(state: &State<AppState>) -> Result<(), String> {
    if !cursor_sync::is_logged_in() {
        return Ok(());
    }
    let synced = cursor_sync::maybe_auto_sync()?;
    if synced {
        reload_cursor_sources(state)?;
    }
    Ok(())
}

#[tauri::command]
pub fn cursor_login(session_token: String, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let validation = cursor_sync::validate_cursor_session(&session_token);
    if !validation.valid {
        return Err(validation.error.unwrap_or_else(|| "Cursor 登录验证失败".to_string()));
    }

    cursor_sync::save_credentials(&session_token)?;
    let sync = cursor_sync::sync_cursor_cache();
    if !sync.synced {
        return Err(sync.error.unwrap_or_else(|| "Cursor 同步失败".to_string()));
    }

    ensure_cursor_source_registered(&state)?;
    save_sources(&state)
}

#[tauri::command]
pub fn cursor_sync(state: State<AppState>) -> Result<SyncCursorResult, String> {
    let result = cursor_sync::sync_cursor_cache();
    if result.synced {
        ensure_cursor_source_registered(&state)?;
        reload_cursor_sources(&state)?;
        let _ = save_sources(&state);
    }
    Ok(result)
}

#[tauri::command]
pub fn cursor_status(state: State<AppState>) -> Result<CursorStatus, String> {
    let logged_in = cursor_sync::is_logged_in();
    let cache_path = utils::get_cursor_cache_dir().ok().map(|p| p.to_string_lossy().to_string());
    let last_sync = cursor_sync::cache_last_modified()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    let record_count = {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        sources
            .iter()
            .find(|s| matches!(s.db_type, DbType::Cursor))
            .and_then(|s| s.source.get_record_count().ok())
            .unwrap_or(0)
    };

    let membership_type = None;

    Ok(CursorStatus {
        logged_in,
        last_sync,
        record_count,
        cache_path,
        membership_type,
    })
}

#[tauri::command]
pub fn cursor_logout(clear_cache: bool, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    cursor_sync::clear_credentials()?;

    if clear_cache {
        if let Ok(dir) = utils::get_cursor_cache_dir() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    sources.retain(|s| !matches!(s.db_type, DbType::Cursor));
    drop(sources);

    save_sources(&state)
}
