use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::services::data_source::create_source_entry;

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

    for path in &paths {
        match create_source_entry(path) {
            Ok(entry) => {
                log::info!("[DB] 自动加载: {} ({})", path, entry.db_type.label());
                sources.push(entry);
            }
            Err(e) => log::error!("[DB] 加载失败 {}: {}", path, e),
        }
    }

    if sources.is_empty() {
        return Ok(Vec::new());
    }

    // 刷新定价引擎
    refresh_pricing(&state)?;

    let info: Vec<SourceInfo> = sources.iter().map(|s| s.to_info()).collect();
    save_paths(&state, &info);
    Ok(info)
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
    let sources = state.data_sources.read().map_err(|e| e.to_string())?;
    if sources.is_empty() {
        return Ok(RefreshResult { has_new: false, record_count: None });
    }

    let current_max = sources.iter()
        .filter_map(|s| s.source.get_latest_timestamp())
        .max();
    let prev_max = *state.db_latest_timestamp.lock().map_err(|e| e.to_string())?;

    if current_max != prev_max {
        *state.db_latest_timestamp.lock().map_err(|e| e.to_string())? = current_max;
        let count: i64 = sources.iter()
            .filter_map(|s| s.source.get_record_count().ok())
            .sum();
        Ok(RefreshResult { has_new: true, record_count: Some(count) })
    } else {
        Ok(RefreshResult { has_new: false, record_count: None })
    }
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

    for entry in sources.iter() {
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
    // Windows 的 canonicalize 会返回 \\?\UNC 前缀，去掉它
    #[cfg(target_os = "windows")]
    {
        let s = canonical.to_string_lossy().to_string();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(std::path::PathBuf::from(stripped));
        }
    }
    Ok(canonical)
}

fn refresh_pricing(state: &State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
    match pricing.refresh(&app_db) {
        Ok(()) => log::info!("[DB] 定价引擎刷新成功, 模型数={}", pricing.size()),
        Err(e) => log::error!("[DB] 定价引擎刷新失败: {}", e),
    }
    Ok(())
}

fn save_paths(state: &State<AppState>, info: &[SourceInfo]) {
    let paths: Vec<&str> = info.iter().map(|s| s.path.as_str()).collect();
    if let Ok(json) = serde_json::to_string(&paths) {
        if let Ok(app_db) = state.app_db.lock() {
            if let Err(e) = app_db.set_setting("last_db_paths", &json) {
                log::error!("[DB] 保存数据库路径失败: {}", e);
            }
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPaths {
    pub cc_switch: Option<String>,
    pub opencode: Option<String>,
}

#[tauri::command]
pub fn get_default_paths() -> Result<DefaultPaths, String> {
    let cc_switch = crate::utils::get_default_db_path().ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()));
    let opencode = crate::utils::get_default_opencode_db_path().ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()));
    Ok(DefaultPaths { cc_switch, opencode })
}
