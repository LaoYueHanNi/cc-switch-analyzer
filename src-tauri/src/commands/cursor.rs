use tauri::State;

use crate::AppState;
use crate::models::SourceInfo;
use crate::services::cursor_attribution::{
    AttributionTokenStats, CursorCsvPreviewPage, OverrideAction,
};
use crate::services::cursor_hook_backup::{self, HookBackupResult};
use crate::services::cursor_local_hook;
use crate::services::cursor_local_hook::HookAlert;
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
    pub attribution_enabled: bool,
    pub hook_installed: bool,
    pub local_event_count: i64,
    pub attribution_hint: String,
    pub attribution_stats: AttributionTokenStats,
    pub sync_lookback: String,
    pub hook_backup_period: String,
    pub hook_backup_count: i64,
    pub hook_last_backup_at: Option<i64>,
    pub hook_alert: Option<HookAlert>,
}

fn cursor_cache_dir_str() -> Result<String, String> {
    Ok(utils::get_cursor_cache_dir()?.to_string_lossy().to_string())
}

fn build_cursor_status(state: &State<AppState>) -> Result<CursorStatus, String> {
    let logged_in = cursor_sync::is_logged_in();
    let cache_path = utils::get_cursor_cache_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    let last_sync = cursor_sync::cache_last_modified()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    // 启动竞态：前端可能在 auto_load 完成前调 status；CSV 已存在但内存源未 open 时补载
    if logged_in {
        let csv_ready = utils::get_cursor_usage_csv_path()
            .map(|p| p.exists())
            .unwrap_or(false);
        if csv_ready {
            let need_load = {
                let sources = state.data_sources.read().map_err(|e| e.to_string())?;
                !sources.iter().any(|s| {
                    matches!(s.db_type, DbType::Cursor)
                        && s.source.get_record_count().ok().unwrap_or(0) > 0
                })
            };
            if need_load {
                let _ = ensure_cursor_source_registered(state);
            }
        }
    }

    let record_count = {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        sources
            .iter()
            .find(|s| matches!(s.db_type, DbType::Cursor))
            .and_then(|s| s.source.get_record_count().ok())
            .unwrap_or(0)
    };

    let attribution_enabled = cursor_local_hook::is_attribution_enabled();
    let hook_installed = cursor_local_hook::is_hook_installed();
    let local_event_count = cursor_local_hook::local_event_count() as i64;
    let attribution_hint = cursor_local_hook::attribution_hint(attribution_enabled);
    let heartbeat = cursor_local_hook::read_hook_heartbeat();
    let hook_alert = cursor_local_hook::hook_alert(attribution_enabled, hook_installed, heartbeat.as_ref());

    let attribution_stats = {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        sources
            .iter()
            .find(|s| matches!(s.db_type, DbType::Cursor))
            .and_then(|s| s.source.get_cursor_attribution_stats())
            .unwrap_or_default()
    };

    let hook_backup = cursor_hook_backup::backup_status();

    Ok(CursorStatus {
        logged_in,
        last_sync,
        record_count,
        cache_path,
        membership_type: None,
        attribution_enabled,
        hook_installed,
        local_event_count,
        attribution_hint,
        attribution_stats,
        sync_lookback: cursor_sync::get_sync_lookback().as_str().to_string(),
        hook_backup_period: hook_backup.period,
        hook_backup_count: hook_backup.backup_count,
        hook_last_backup_at: hook_backup.last_backup_at,
        hook_alert,
    })
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

fn ensure_cursor_source_registered(state: &State<AppState>) -> Result<(), String> {
    let cache_str = cursor_cache_dir_str()?;
    let csv_path = utils::get_cursor_usage_csv_path()?;
    if !csv_path.exists() {
        return Ok(());
    }

    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    if let Some(entry) = sources
        .iter_mut()
        .find(|s| matches!(s.db_type, DbType::Cursor))
    {
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
        return Err(validation
            .error
            .unwrap_or_else(|| "Cursor 登录验证失败".to_string()));
    }

    cursor_sync::save_credentials(&session_token)?;
    let sync = cursor_sync::sync_cursor_cache();
    if !sync.synced {
        return Err(sync
            .error
            .unwrap_or_else(|| "Cursor 同步失败".to_string()));
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
    build_cursor_status(&state)
}

#[tauri::command]
pub fn cursor_preview_csv(
    page: Option<usize>,
    page_size: Option<usize>,
    filtered_only: Option<bool>,
    model: Option<String>,
    state: State<AppState>,
) -> Result<CursorCsvPreviewPage, String> {
    let page = page.unwrap_or(1);
    let page_size = page_size.unwrap_or(50);
    let filtered_only = filtered_only.unwrap_or(false);
    let model_filter = model.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let csv_ready = utils::get_cursor_usage_csv_path()
        .map(|p| p.exists())
        .unwrap_or(false);
    if !csv_ready {
        return Ok(CursorCsvPreviewPage {
            items: Vec::new(),
            total: 0,
            page: page.max(1),
            page_size: page_size.clamp(1, 100),
            available_models: Vec::new(),
        });
    }

    ensure_cursor_source_registered(&state)?;

    {
        let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
        if let Some(entry) = sources
            .iter_mut()
            .find(|s| matches!(s.db_type, DbType::Cursor))
        {
            entry.source.refresh_cursor_local_events();
        }
    }

    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    sources
        .iter()
        .find(|s| matches!(s.db_type, DbType::Cursor))
        .and_then(|s| {
            s.source
                .get_cursor_csv_preview(page, page_size, filtered_only, model_filter)
        })
        .ok_or_else(|| "Cursor 数据源未加载".to_string())
}

#[tauri::command]
pub fn cursor_set_attribution_override(
    row_key: String,
    action: String,
    created_at: i64,
    model: String,
    state: State<AppState>,
) -> Result<CursorStatus, String> {
    let action = match action.trim().to_lowercase().as_str() {
        "keep" => OverrideAction::Keep,
        "filter" => OverrideAction::Filter,
        other => return Err(format!("无效改判 action: {}", other)),
    };
    if row_key.trim().is_empty() {
        return Err("rowKey 不能为空".to_string());
    }

    ensure_cursor_source_registered(&state)?;
    {
        let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
        let entry = sources
            .iter_mut()
            .find(|s| matches!(s.db_type, DbType::Cursor))
            .ok_or_else(|| "Cursor 数据源未加载".to_string())?;
        entry
            .source
            .set_cursor_attribution_override(&row_key, action, created_at, &model)?;
    }
    build_cursor_status(&state)
}

#[tauri::command]
pub fn cursor_clear_attribution_override(
    row_key: String,
    state: State<AppState>,
) -> Result<CursorStatus, String> {
    if row_key.trim().is_empty() {
        return Err("rowKey 不能为空".to_string());
    }
    ensure_cursor_source_registered(&state)?;
    {
        let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
        let entry = sources
            .iter_mut()
            .find(|s| matches!(s.db_type, DbType::Cursor))
            .ok_or_else(|| "Cursor 数据源未加载".to_string())?;
        entry
            .source
            .clear_cursor_attribution_override(&row_key)?;
    }
    build_cursor_status(&state)
}

#[tauri::command]
pub fn cursor_toggle_attribution(
    enabled: bool,
    state: State<AppState>,
) -> Result<CursorStatus, String> {
    cursor_local_hook::set_attribution_enabled(enabled)?;
    let hint = if enabled {
        cursor_local_hook::install_hooks()?
    } else {
        cursor_local_hook::uninstall_hooks()?
    };
    log::info!("[CURSOR] attribution toggle enabled={} hint={}", enabled, hint);

    // 有 CSV 时重载以应用过滤
    if utils::get_cursor_usage_csv_path()
        .map(|p| p.exists())
        .unwrap_or(false)
    {
        let _ = ensure_cursor_source_registered(&state);
        let _ = reload_cursor_sources(&state);
        let _ = save_sources(&state);
    }

    let mut status = build_cursor_status(&state)?;
    if !hint.is_empty() {
        status.attribution_hint = hint;
    }
    Ok(status)
}

#[tauri::command]
pub fn cursor_set_sync_lookback(
    lookback: String,
    state: State<AppState>,
) -> Result<CursorStatus, String> {
    let parsed = cursor_sync::set_sync_lookback(&lookback)?;
    log::info!("[CURSOR] sync lookback set to {}", parsed.as_str());
    build_cursor_status(&state)
}

#[tauri::command]
pub fn cursor_set_hook_backup_period(
    period: String,
    state: State<AppState>,
) -> Result<CursorStatus, String> {
    let parsed = cursor_hook_backup::set_hook_backup_period(&period)?;
    log::info!("[CURSOR] hook backup period set to {}", parsed.as_str());
    build_cursor_status(&state)
}

#[tauri::command]
pub fn cursor_backup_hooks_now() -> Result<HookBackupResult, String> {
    let result = cursor_hook_backup::backup_now()?;
    log::info!(
        "[CURSOR] hook backup now backed_up={} msg={}",
        result.backed_up,
        result.message
    );
    Ok(result)
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
