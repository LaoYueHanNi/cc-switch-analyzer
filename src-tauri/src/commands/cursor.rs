use tauri::State;

use crate::AppState;
use crate::models::SourceInfo;
use crate::services::cursor_attribution::{
    AttributionTokenStats, CursorCsvPreviewPage, CursorCsvPreviewRow, OverrideAction, TokenQuad,
};
use crate::services::cursor_hook_backup::{self, HookBackupResult};
use crate::services::cursor_local_hook;
use crate::services::cursor_local_hook::HookAlert;
use crate::services::cursor_sync::{self, SyncCursorResult};
use crate::services::data_source::{create_source_entry, DbType};
use crate::utils;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorAccountStatus {
    pub user_id: String,
    pub path: String,
    pub record_count: i64,
    pub last_sync: Option<i64>,
    pub is_sync_account: bool,
    pub enabled: bool,
    pub source_id: String,
    pub attribution_stats: AttributionTokenStats,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorStatus {
    pub logged_in: bool,
    /// 当前可同步账号的 userId（脱敏前原始值）
    pub user_id: Option<String>,
    pub last_sync: Option<i64>,
    /// 全部已加载 Cursor 账号记录合计
    pub record_count: i64,
    pub cache_path: Option<String>,
    pub membership_type: Option<String>,
    pub attribution_enabled: bool,
    pub hook_installed: bool,
    pub local_event_count: i64,
    pub attribution_hint: String,
    pub attribution_stats: AttributionTokenStats,
    /// 本机归因过滤起始时刻（Unix 秒，东八区语义）
    pub attribution_filter_start: i64,
    pub sync_lookback: String,
    pub hook_backup_period: String,
    pub hook_backup_count: i64,
    pub hook_last_backup_at: Option<i64>,
    pub hook_alert: Option<HookAlert>,
    pub accounts: Vec<CursorAccountStatus>,
}

fn merge_token_quad(into: &mut TokenQuad, from: &TokenQuad) {
    into.input += from.input;
    into.output += from.output;
    into.cache_read += from.cache_read;
    into.cache_creation += from.cache_creation;
}

fn merge_attr_stats(into: &mut AttributionTokenStats, from: &AttributionTokenStats) {
    merge_token_quad(&mut into.csv_total, &from.csv_total);
    merge_token_quad(&mut into.filtered_out, &from.filtered_out);
}

fn active_sync_user_id() -> Option<String> {
    let creds = cursor_sync::load_credentials()?;
    Some(cursor_sync::account_dir_key_from_creds(&creds))
}

fn path_norm(p: &str) -> String {
    std::path::Path::new(p)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(p))
        .to_string_lossy()
        .to_string()
        .to_lowercase()
}

fn paths_equal(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    path_norm(a) == path_norm(b)
}

fn build_cursor_status(state: &State<AppState>) -> Result<CursorStatus, String> {
    let logged_in = cursor_sync::is_logged_in();
    let sync_uid = active_sync_user_id();

    // 有任意账号 CSV 时补注册（含未登录离线账号）
    if utils::any_cursor_usage_csv_exists() {
        let need_load = {
            let sources = state.data_sources.read().map_err(|e| e.to_string())?;
            !sources.iter().any(|s| matches!(s.db_type, DbType::Cursor))
        };
        if need_load {
            let _ = ensure_all_cursor_sources_registered(state);
        }
    }

    let active_cache = cursor_sync::try_active_cursor_cache_dir()
        .map(|p| p.to_string_lossy().to_string());

    let last_sync = cursor_sync::cache_last_modified()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    let attribution_enabled = cursor_local_hook::is_attribution_enabled();
    let hook_installed = cursor_local_hook::is_hook_installed();
    let local_event_count = cursor_local_hook::local_event_count() as i64;
    let mut attribution_hint = cursor_local_hook::attribution_hint(attribution_enabled);
    if attribution_enabled && !attribution_hint.is_empty() && !attribution_hint.contains("多账号") {
        attribution_hint = format!("{}（多账号共用本机 Hook）", attribution_hint);
    }
    let heartbeat = cursor_local_hook::read_hook_heartbeat();
    let hook_alert =
        cursor_local_hook::hook_alert(attribution_enabled, hook_installed, heartbeat.as_ref());

    let mut record_count: i64 = 0;
    let mut attribution_stats = AttributionTokenStats::default();
    let mut accounts: Vec<CursorAccountStatus> = Vec::new();

    {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        for entry in sources.iter().filter(|s| matches!(s.db_type, DbType::Cursor)) {
            let count = entry.source.get_record_count().unwrap_or(0);
            record_count += count;
            let acc_stats = entry
                .source
                .get_cursor_attribution_stats()
                .unwrap_or_default();
            merge_attr_stats(&mut attribution_stats, &acc_stats);
            let user_id = utils::resolve_account_user_id(std::path::Path::new(&entry.path));
            let last = cursor_sync::account_cache_last_modified(std::path::Path::new(&entry.path))
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            let is_sync = sync_uid
                .as_ref()
                .map(|uid| {
                    utils::sanitize_user_id(uid) == utils::sanitize_user_id(&user_id)
                        || sync_uid.as_deref() == Some(user_id.as_str())
                })
                .unwrap_or(false);
            accounts.push(CursorAccountStatus {
                user_id,
                path: entry.path.clone(),
                record_count: count,
                last_sync: last,
                is_sync_account: is_sync && logged_in,
                enabled: entry.enabled,
                source_id: entry.id.clone(),
                attribution_stats: acc_stats,
            });
        }
    }

    // 磁盘上有但尚未注册的账号也展示（启动竞态）
    if let Ok(disk) = utils::list_cursor_account_caches() {
        for acc in disk {
            let path_str = acc.path.to_string_lossy().to_string();
            if accounts.iter().any(|a| paths_equal(&a.path, &path_str)) {
                continue;
            }
            let last = cursor_sync::account_cache_last_modified(&acc.path)
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            let is_sync = sync_uid
                .as_ref()
                .map(|uid| utils::sanitize_user_id(uid) == utils::sanitize_user_id(&acc.user_id))
                .unwrap_or(false);
            accounts.push(CursorAccountStatus {
                user_id: acc.user_id,
                path: path_str,
                record_count: 0,
                last_sync: last,
                is_sync_account: is_sync && logged_in,
                enabled: true,
                source_id: String::new(),
                attribution_stats: AttributionTokenStats::default(),
            });
        }
    }

    accounts.sort_by(|a, b| a.user_id.cmp(&b.user_id));

    let hook_backup = cursor_hook_backup::backup_status();

    Ok(CursorStatus {
        logged_in,
        user_id: sync_uid,
        last_sync,
        record_count,
        cache_path: active_cache.or_else(|| {
            accounts
                .first()
                .map(|a| a.path.clone())
                .or_else(|| {
                    utils::get_cursor_cache_root()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
                })
        }),
        membership_type: None,
        attribution_enabled,
        hook_installed,
        local_event_count,
        attribution_hint,
        attribution_stats,
        attribution_filter_start: cursor_local_hook::get_attribution_filter_start(),
        sync_lookback: cursor_sync::get_sync_lookback().as_str().to_string(),
        hook_backup_period: hook_backup.period,
        hook_backup_count: hook_backup.backup_count,
        hook_last_backup_at: hook_backup.last_backup_at,
        hook_alert,
        accounts,
    })
}

/// 扫描并注册/重载全部账号缓存目录
pub fn ensure_all_cursor_sources_registered(state: &State<AppState>) -> Result<(), String> {
    let _ = utils::migrate_legacy_cursor_cache(active_sync_user_id().as_deref());
    let caches = utils::list_cursor_account_caches()?;
    if caches.is_empty() {
        return Ok(());
    }

    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    for acc in caches {
        let cache_str = acc.path.to_string_lossy().to_string();
        if let Some(entry) = sources
            .iter_mut()
            .find(|s| matches!(s.db_type, DbType::Cursor) && paths_equal(&s.path, &cache_str))
        {
            if let Err(e) = entry.source.open(&cache_str) {
                log::warn!("[CURSOR] 重载账号缓存失败 {}: {}", cache_str, e);
            }
            continue;
        }
        match create_source_entry(&cache_str) {
            Ok(entry) => {
                log::info!("[CURSOR] 注册数据源: {} (user={})", cache_str, acc.user_id);
                sources.push(entry);
            }
            Err(e) => log::warn!("[CURSOR] 注册失败 {}: {}", cache_str, e),
        }
    }
    Ok(())
}

pub fn reload_cursor_sources(state: &State<AppState>) -> Result<(), String> {
    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    for entry in sources.iter_mut() {
        if matches!(entry.db_type, DbType::Cursor) {
            let path = entry.path.clone();
            if let Err(e) = entry.source.open(&path) {
                log::warn!("[CURSOR] reload {} failed: {}", path, e);
            }
        }
    }
    Ok(())
}

fn save_sources(state: &State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    let info: Vec<SourceInfo> = sources.iter().map(|s| s.to_info()).collect();
    crate::commands::database::save_paths_public(state, &info);
    Ok(info)
}

/// 查询前自动同步 Cursor 缓存并在有更新时重载数据源
pub fn sync_and_reload_if_needed(state: &State<AppState>) -> Result<(), String> {
    // 离线账号也要确保已注册
    if utils::any_cursor_usage_csv_exists() {
        let _ = ensure_all_cursor_sources_registered(state);
    }
    if !cursor_sync::is_logged_in() {
        return Ok(());
    }
    let synced = cursor_sync::maybe_auto_sync()?;
    if synced {
        ensure_all_cursor_sources_registered(state)?;
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

    ensure_all_cursor_sources_registered(&state)?;
    save_sources(&state)
}

#[tauri::command]
pub fn cursor_sync(state: State<AppState>) -> Result<SyncCursorResult, String> {
    let result = cursor_sync::sync_cursor_cache();
    if result.synced {
        ensure_all_cursor_sources_registered(&state)?;
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
    cache_path: Option<String>,
    user_id: Option<String>,
    state: State<AppState>,
) -> Result<CursorCsvPreviewPage, String> {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size.unwrap_or(50).clamp(1, 100);
    let filtered_only = filtered_only.unwrap_or(false);
    let model_filter = model.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let path_filter = cache_path.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let uid_filter = user_id.as_deref().map(str::trim).filter(|s| !s.is_empty());

    if !utils::any_cursor_usage_csv_exists() {
        return Ok(CursorCsvPreviewPage {
            items: Vec::new(),
            total: 0,
            page,
            page_size,
            available_models: Vec::new(),
        });
    }

    ensure_all_cursor_sources_registered(&state)?;

    {
        let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
        for entry in sources.iter_mut().filter(|s| matches!(s.db_type, DbType::Cursor)) {
            entry.source.refresh_cursor_local_events();
        }
    }

    // 聚合各账号预览：各取大页再合并排序分页
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    let mut all_items: Vec<CursorCsvPreviewRow> = Vec::new();
    let mut available_models: Vec<String> = Vec::new();

    for entry in sources.iter().filter(|s| matches!(s.db_type, DbType::Cursor)) {
        if let Some(pf) = path_filter {
            if !paths_equal(&entry.path, pf) {
                continue;
            }
        }
        let resolved_uid = utils::resolve_account_user_id(std::path::Path::new(&entry.path));
        if let Some(uid) = uid_filter {
            if resolved_uid != uid
                && utils::sanitize_user_id(&resolved_uid) != utils::sanitize_user_id(uid)
            {
                continue;
            }
        }
        // 拉全量再本地过滤（预览量通常不大）
        if let Some(preview) =
            entry
                .source
                .get_cursor_csv_preview(1, 10_000, filtered_only, None)
        {
            available_models.extend(preview.available_models);
            for mut row in preview.items {
                if row.cache_path.is_none() {
                    row.cache_path = Some(entry.path.clone());
                }
                if row.user_id.is_none() {
                    row.user_id = Some(resolved_uid.clone());
                }
                all_items.push(row);
            }
        }
    }
    drop(sources);

    available_models.sort();
    available_models.dedup();

    if let Some(model) = model_filter {
        all_items.retain(|r| r.model == model);
    }

    all_items.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.user_id.cmp(&b.user_id))
            .then_with(|| a.row_key.cmp(&b.row_key))
    });

    let total = all_items.len();
    let start = (page - 1).saturating_mul(page_size);
    let items = if start >= total {
        Vec::new()
    } else {
        all_items[start..(start + page_size).min(total)].to_vec()
    };

    Ok(CursorCsvPreviewPage {
        items,
        total,
        page,
        page_size,
        available_models,
    })
}

fn find_cursor_source_mut<'a>(
    sources: &'a mut [crate::services::data_source::SourceEntry],
    cache_path: Option<&str>,
    user_id: Option<&str>,
) -> Result<&'a mut crate::services::data_source::SourceEntry, String> {
    if let Some(path) = cache_path.map(str::trim).filter(|s| !s.is_empty()) {
        return sources
            .iter_mut()
            .find(|s| matches!(s.db_type, DbType::Cursor) && paths_equal(&s.path, path))
            .ok_or_else(|| format!("未找到 Cursor 数据源: {}", path));
    }
    if let Some(uid) = user_id.map(str::trim).filter(|s| !s.is_empty()) {
        let sanitized = utils::sanitize_user_id(uid);
        return sources
            .iter_mut()
            .find(|s| {
                if !matches!(s.db_type, DbType::Cursor) {
                    return false;
                }
                let resolved = utils::resolve_account_user_id(std::path::Path::new(&s.path));
                resolved == uid || utils::sanitize_user_id(&resolved) == sanitized
            })
            .ok_or_else(|| format!("未找到 Cursor 账号: {}", uid));
    }
    // 回退：仅一个 Cursor 源时可用
    let cursor_count = sources
        .iter()
        .filter(|s| matches!(s.db_type, DbType::Cursor))
        .count();
    if cursor_count == 1 {
        return sources
            .iter_mut()
            .find(|s| matches!(s.db_type, DbType::Cursor))
            .ok_or_else(|| "Cursor 数据源未加载".to_string());
    }
    Err("多账号时改判必须指定 cachePath 或 userId".to_string())
}

#[tauri::command]
pub fn cursor_set_attribution_override(
    row_key: String,
    action: String,
    created_at: i64,
    model: String,
    cache_path: Option<String>,
    user_id: Option<String>,
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

    ensure_all_cursor_sources_registered(&state)?;
    {
        let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
        let entry = find_cursor_source_mut(
            &mut sources,
            cache_path.as_deref(),
            user_id.as_deref(),
        )?;
        entry
            .source
            .set_cursor_attribution_override(&row_key, action, created_at, &model)?;
    }
    build_cursor_status(&state)
}

#[tauri::command]
pub fn cursor_clear_attribution_override(
    row_key: String,
    cache_path: Option<String>,
    user_id: Option<String>,
    state: State<AppState>,
) -> Result<CursorStatus, String> {
    if row_key.trim().is_empty() {
        return Err("rowKey 不能为空".to_string());
    }
    ensure_all_cursor_sources_registered(&state)?;
    {
        let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
        let entry = find_cursor_source_mut(
            &mut sources,
            cache_path.as_deref(),
            user_id.as_deref(),
        )?;
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

    if utils::any_cursor_usage_csv_exists() {
        let _ = ensure_all_cursor_sources_registered(&state);
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
pub fn cursor_set_attribution_filter_start(
    epoch: i64,
    state: State<AppState>,
) -> Result<CursorStatus, String> {
    let parsed = cursor_local_hook::set_attribution_filter_start(epoch)?;
    log::info!("[CURSOR] attribution filter start set to {}", parsed);

    if utils::any_cursor_usage_csv_exists() {
        let _ = ensure_all_cursor_sources_registered(&state);
        {
            let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
            for entry in sources.iter_mut().filter(|s| matches!(s.db_type, DbType::Cursor)) {
                entry.source.refresh_cursor_local_events();
            }
        }
    }

    build_cursor_status(&state)
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
pub fn cursor_merge_hooks_now() -> Result<crate::services::cursor_hook_merge::HookMergeResult, String> {
    let result = cursor_hook_backup::merge_hooks_now()?;
    log::info!(
        "[CURSOR] hook merge now merged={} msg={}",
        result.merged,
        result.message
    );
    Ok(result)
}

#[tauri::command]
pub fn cursor_logout(clear_cache: bool, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let active_dir = cursor_sync::try_active_cursor_cache_dir();
    cursor_sync::clear_credentials()?;

    if clear_cache {
        if let Some(ref dir) = active_dir {
            let _ = std::fs::remove_dir_all(dir);
            let path_str = dir.to_string_lossy().to_string();
            let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
            sources.retain(|s| {
                !(matches!(s.db_type, DbType::Cursor) && paths_equal(&s.path, &path_str))
            });
        }
    }
    // 默认保留所有 Cursor 源，登出后仍可查询离线账号

    save_sources(&state)
}
