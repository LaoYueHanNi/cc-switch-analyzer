use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::services::data_source::{create_source_entry, SourceEntry};
use crate::services::pipeline::run_streaming_dedup;

// ========== 数据库操作命令 ==========

#[tauri::command]
pub fn auto_load_database(state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let paths_json = app_db.get_setting("last_db_paths");
    drop(app_db);

    let paths: Vec<String> = paths_json
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    if paths.is_empty() {
        // 兼容旧版：尝试 last_db_path（已废弃，由 last_db_paths 替代）
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let single = app_db.get_setting("last_db_path");
        drop(app_db);
        if let Some(p) = single {
            if std::path::Path::new(&p).exists() {
                return auto_load_paths(&state, vec![p]);
            }
        }
        // 自动探测默认路径
        let mut defaults = Vec::new();
        if let Some(p) = crate::utils::get_default_db_path().ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .filter(|p| std::path::Path::new(p).exists())
        {
            defaults.push(p);
        }
        if let Some(p) = crate::utils::get_default_opencode_db_path().ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .filter(|p| std::path::Path::new(p).exists())
        {
            defaults.push(p);
        }
        if let Some(p) = crate::utils::get_default_ai_proxy_db_path().ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .filter(|p| std::path::Path::new(p).exists())
        {
            defaults.push(p);
        }
        if crate::utils::has_cursor_byok_usage() {
            if let Ok(p) = crate::utils::get_default_cursor_byok_history_path() {
                defaults.push(p.to_string_lossy().to_string());
            }
        }
        if cursor_should_auto_load() {
            if let Ok(caches) = crate::utils::list_cursor_account_caches() {
                for acc in caches {
                    defaults.push(acc.path.to_string_lossy().to_string());
                }
            }
        }
        if !defaults.is_empty() {
            return auto_load_paths(&state, defaults);
        }
        log::info!("[DB] 无可加载的数据库");
        return Ok(Vec::new());
    }

    let existing: Vec<String> = paths.into_iter().filter(|p| std::path::Path::new(p).exists()).collect();
    if existing.is_empty() {
        log::info!("[DB] 记忆的数据库路径均不存在");
        return Ok(Vec::new());
    }

    auto_load_paths(&state, existing)
}

fn auto_load_paths(state: &State<AppState>, paths: Vec<String>) -> Result<Vec<SourceInfo>, String> {
    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    sources.clear();

    let cache_root = crate::utils::get_cursor_cache_root()
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    for path in &paths {
        // 丢弃无效旧根路径（扁平 cursor-cache 且无 usage.csv）
        if let Some(ref root) = cache_root {
            if path == root || path_equals_ignore_slash(path, root) {
                let csv = std::path::Path::new(path).join("usage.csv");
                if !csv.is_file() {
                    log::info!("[DB] 跳过旧 Cursor 根路径: {}", path);
                    continue;
                }
            }
        }
        match create_source_entry(path) {
            Ok(entry) => {
                log::info!("[DB] 自动加载: {} ({})", path, entry.db_type.label());
                sources.push(entry);
            }
            Err(e) => log::error!("[DB] 加载失败 {}: {}", path, e),
        }
    }

    // 扫描磁盘上全部账号缓存并补注册
    let _ = crate::utils::migrate_legacy_cursor_cache(
        crate::services::cursor_sync::load_credentials()
            .map(|c| crate::services::cursor_sync::account_dir_key_from_creds(&c))
            .as_deref(),
    );
    if let Ok(caches) = crate::utils::list_cursor_account_caches() {
        for acc in caches {
            let cache_str = acc.path.to_string_lossy().to_string();
            let already = sources.iter().any(|s| {
                matches!(s.db_type, crate::services::data_source::DbType::Cursor)
                    && path_equals_ignore_slash(&s.path, &cache_str)
            });
            if already {
                continue;
            }
            match create_source_entry(&cache_str) {
                Ok(entry) => {
                    log::info!(
                        "[DB] 自动加载 Cursor 账号: {} ({})",
                        cache_str,
                        acc.user_id
                    );
                    sources.push(entry);
                }
                Err(e) => log::error!("[DB] Cursor 账号加载失败 {}: {}", cache_str, e),
            }
        }
    }

    if sources.is_empty() {
        return Ok(Vec::new());
    }

    // 恢复 disabled 状态
    let disabled = load_disabled_paths(&state);
    for entry in sources.iter_mut() {
        if disabled.contains(&entry.path) {
            entry.enabled = false;
        }
    }

    // 刷新定价引擎
    refresh_pricing(state)?;

    let info: Vec<SourceInfo> = sources.iter().map(|s| s.to_info()).collect();
    save_paths(&state, &info);
    Ok(info)
}

fn path_equals_ignore_slash(a: &str, b: &str) -> bool {
    let na = a.replace('\\', "/").trim_end_matches('/').to_lowercase();
    let nb = b.replace('\\', "/").trim_end_matches('/').to_lowercase();
    na == nb
}

#[tauri::command]
pub fn load_database(file_path: String, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let canonical = validate_db_path(&file_path)?;
    let canonical_str = canonical.to_string_lossy().to_string();
    log::info!("[DB] load_database: {}", canonical_str);

    let entry = create_source_entry(&canonical_str)?;
    log::info!("[DB] 打开成功 ({})", entry.db_type.label());

    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    sources.clear();
    sources.push(entry);

    drop(sources);
    refresh_pricing(&state)?;

    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    let info: Vec<SourceInfo> = sources.iter().map(|s| s.to_info()).collect();
    save_paths(&state, &info);
    Ok(info)
}

#[tauri::command]
pub fn add_database(file_path: String, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let canonical = validate_db_path(&file_path)?;
    let canonical_str = canonical.to_string_lossy().to_string();
    log::info!("[DB] add_database: {}", canonical_str);

    // 检查是否已加载，避免重复
    {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        if sources.iter().any(|s| s.path == canonical_str) {
            return Err(format!("数据库已加载: {}", canonical_str));
        }
    }

    let entry = create_source_entry(&canonical_str)?;
    log::info!("[DB] 添加成功 ({})", entry.db_type.label());

    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    sources.push(entry);

    drop(sources);
    refresh_pricing(&state)?;

    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    let info: Vec<SourceInfo> = sources.iter().map(|s| s.to_info()).collect();
    save_paths(&state, &info);
    Ok(info)
}

#[tauri::command]
pub fn remove_database(source_id: String, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    let before = sources.len();
    sources.retain(|s| s.id != source_id);
    if sources.len() == before {
        return Err("数据源不存在".to_string());
    }
    log::info!("[DB] 移除数据源: {}", source_id);

    drop(sources);
    refresh_pricing(&state)?;

    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    let info: Vec<SourceInfo> = sources.iter().map(|s| s.to_info()).collect();
    save_paths(&state, &info);
    Ok(info)
}

#[tauri::command]
pub fn list_databases(state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    Ok(sources.iter().map(|s| s.to_info()).collect())
}

#[tauri::command]
pub fn refresh_database(state: State<AppState>) -> Result<RefreshResult, String> {
    let _ = crate::commands::cursor::sync_and_reload_if_needed(&state);

    // Phase 1: 快速检查文件 mtime，无变化则直接返回
    {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        if sources.is_empty() {
            return Ok(RefreshResult { has_new: false, record_count: None });
        }

        let mut mtimes = state.db_file_mtimes.lock().map_err(|e| e.to_string())?;
        let mut any_changed = false;
        for entry in sources.iter() {
            let mtime = source_mtime(&entry.path, &entry.db_type)
                .and_then(|m| m.modified().ok());
            let prev = mtimes.get(&entry.path).copied();
            if mtime != prev {
                any_changed = true;
                if let Some(t) = mtime {
                    mtimes.insert(entry.path.clone(), t);
                }
            }
        }
        drop(sources);
        drop(mtimes);

        if !any_changed {
            return Ok(RefreshResult { has_new: false, record_count: None });
        }
    }

    // Phase 2: 文件有变化，走 SQL 查询确认
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    let current_max = sources.iter()
        .filter_map(|s| s.source.get_latest_timestamp())
        .max();
    let prev_max = *state.db_latest_timestamp.lock().map_err(|e| e.to_string())?;

    if current_max == prev_max {
        return Ok(RefreshResult { has_new: false, record_count: None });
    }

    // Phase 3: 增量流式更新请求缓存
    {
        let since = *state.db_latest_timestamp.lock().map_err(|e| e.to_string())?;
        let raw_records = run_streaming_dedup(&sources, since);
        let new_tokens: Vec<SessionRequestToken> = raw_records.into_iter()
            .map(|(session_id, model, provider_id, created_at, input_tokens, output_tokens, cache_read, cache_creation, _latency, _is_codex)| {
                SessionRequestToken { session_id, model, provider_id, created_at, input_tokens, output_tokens, cache_read, cache_creation }
            })
            .collect();
        let mut cache = state.request_cache.lock().map_err(|e| e.to_string())?;
        let added = cache.merge(new_tokens);
        log::info!("[DB] 增量缓存更新: 新增 {} 条, 缓存总计 {} 条", added, cache.len());
    }

    *state.db_latest_timestamp.lock().map_err(|e| e.to_string())? = current_max;
    let count: i64 = sources.iter()
        .filter_map(|s| s.source.get_record_count().ok())
        .sum();
    Ok(RefreshResult { has_new: true, record_count: Some(count) })
}

#[tauri::command]
pub fn get_filter_options(state: State<AppState>) -> Result<FilterOptions, String> {
    use crate::services::data_source::*;
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    if sources.is_empty() {
        return Ok(FilterOptions {
            providers: Vec::new(),
            models: Vec::new(),
            date_range: DateRange { min: 0, max: 0 },
        });
    }

    let mut all_providers = Vec::new();
    let mut all_models = Vec::new();
    let mut all_ranges = Vec::new();

    for entry in sources.iter().filter(|s| s.enabled) {
        if let Ok(p) = entry.source.get_providers() { all_providers.push(p); }
        if let Ok(m) = entry.source.get_models() { all_models.push(m); }
        if let Ok(r) = entry.source.get_date_range() { all_ranges.push(r); }
    }

    Ok(FilterOptions {
        providers: union_providers(all_providers),
        models: union_models(all_models),
        date_range: merge_date_range(all_ranges),
    })
}

// ========== 辅助函数 ==========

fn validate_db_path(file_path: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(file_path);
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| format!("路径不存在或无法访问: {} ({})", file_path, e))?;
    if canonical.is_dir() {
        // 目录型数据源（Cursor 缓存 / Cursor-BYOK history），类型由 create_source_entry 判定
        return strip_windows_unc(&canonical);
    }
    if !canonical.is_file() {
        return Err(format!("路径不是文件: {}", canonical.display()));
    }
    let valid_extensions = ["db", "sqlite", "sqlite3"];
    match canonical.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if valid_extensions.contains(&ext.to_lowercase().as_str()) => {}
        other => {
            return Err(format!(
                "不支持的文件扩展名: {:?}，仅支持 .db/.sqlite/.sqlite3",
                other
            ));
        }
    }
    strip_windows_unc(&canonical)
}

/// Windows 的 canonicalize 会返回 \\?\UNC 前缀，去掉它
fn strip_windows_unc(canonical: &std::path::Path) -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let s = canonical.to_string_lossy().to_string();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(std::path::PathBuf::from(stripped));
        }
    }
    Ok(canonical.to_path_buf())
}

fn refresh_pricing(state: &AppState) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    pricing.refresh(&app_db).map_err(|e| {
        log::error!("[DB] 定价引擎刷新失败: {}", e);
        e
    })?;
    log::info!("[DB] 定价引擎刷新成功, 模型数={}", pricing.size());
    Ok(())
}

fn save_paths(state: &State<AppState>, info: &[SourceInfo]) {
    save_paths_public(state, info);
}

pub fn save_paths_public(state: &State<AppState>, info: &[SourceInfo]) {
    let paths: Vec<&str> = info.iter().map(|s| s.path.as_str()).collect();
    if let Ok(json) = serde_json::to_string(&paths) {
        if let Ok(app_db) = state.app_db.lock() {
            if let Err(e) = app_db.set_setting("last_db_paths", &json) {
                log::error!("[DB] 保存数据库路径失败: {}", e);
            }
        }
    }
}

fn save_disabled_paths(state: &State<AppState>, sources: &[SourceEntry]) {
    let disabled: Vec<&str> = sources.iter()
        .filter(|s| !s.enabled)
        .map(|s| s.path.as_str())
        .collect();
    if let Ok(app_db) = state.app_db.lock() {
        if let Ok(json) = serde_json::to_string(&disabled) {
            let _ = app_db.set_setting("disabled_source_paths", &json);
        }
    }
}

fn load_disabled_paths(state: &State<AppState>) -> Vec<String> {
    state.app_db.lock().ok()
        .and_then(|db| db.get_setting("disabled_source_paths"))
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn toggle_database(source_id: String, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    let entry = sources.iter_mut().find(|s| s.id == source_id)
        .ok_or("数据源不存在")?;
    entry.enabled = !entry.enabled;
    log::info!("[DB] toggle {} → enabled={}", source_id, entry.enabled);
    drop(sources);

    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    save_disabled_paths(&state, &sources);
    Ok(sources.iter().map(|s| s.to_info()).collect())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPaths {
    pub cc_switch: Option<String>,
    pub opencode: Option<String>,
    pub ai_proxy: Option<String>,
    pub cursor: Option<String>,
    pub cursor_byok: Option<String>,
}

#[tauri::command]
pub fn get_default_paths() -> Result<DefaultPaths, String> {
    let cc_switch = crate::utils::get_default_db_path().ok()
        .map(|p| best_default_path(&p));
    let opencode = crate::utils::get_default_opencode_db_path().ok()
        .map(|p| best_default_path(&p));
    let ai_proxy = crate::utils::get_default_ai_proxy_db_path().ok()
        .map(|p| best_default_path(&p));
    let cursor = crate::utils::get_cursor_cache_dir().ok()
        .map(|p| p.to_string_lossy().to_string());
    let cursor_byok = crate::utils::get_default_cursor_byok_history_path().ok()
        .map(|p| p.to_string_lossy().to_string());
    Ok(DefaultPaths { cc_switch, opencode, ai_proxy, cursor, cursor_byok })
}

fn cursor_should_auto_load() -> bool {
    crate::utils::any_cursor_usage_csv_exists()
}

fn source_mtime(path: &str, db_type: &crate::services::data_source::DbType) -> Option<std::fs::Metadata> {
    use crate::services::data_source::DbType;
    match db_type {
        DbType::Cursor => {
            let csv = std::path::Path::new(path).join("usage.csv");
            std::fs::metadata(csv).ok()
        }
        DbType::CursorByok => {
            let usage = std::path::Path::new(path).join("usage.json");
            std::fs::metadata(usage).ok()
        }
        _ => std::fs::metadata(path).ok(),
    }
}

/// 为文件选择对话框选择最佳默认路径：
/// 文件存在 → 用文件路径；父目录存在 → 用父目录；否则 → home 目录
fn best_default_path(target: &std::path::Path) -> String {
    if target.exists() {
        target.to_string_lossy().to_string()
    } else if let Some(parent) = target.parent() {
        if parent.exists() {
            parent.to_string_lossy().to_string()
        } else {
            dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string())
        }
    } else {
        dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string())
    }
}
