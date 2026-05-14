use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::services::data_source::{create_data_source, detect_db_type};

// ========== 数据库操作命令 ==========

#[tauri::command]
pub fn auto_load_database(state: State<AppState>) -> Result<Option<DatabaseInfo>, String> {
    // 优先使用上次选择的数据库路径
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let remembered = app_db.get_setting("last_db_path");
    let db_path = remembered
        .as_ref()
        .filter(|p| std::path::Path::new(p).exists())
        .cloned()
        .unwrap_or_else(|| {
            crate::utils::get_default_db_path()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_default()
        });
    drop(app_db);

    log::info!("[DB] auto_load_database: path={}", db_path);
    if !std::path::Path::new(&db_path).exists() {
        log::info!("[DB] 数据库不存在");
        return Ok(None);
    }

    let db_type = detect_db_type(&db_path)?;
    log::info!("[DB] 检测到数据库类型: {:?}", db_type);
    let mut ds = state.data_source.write().map_err(|e| e.to_string())?;
    *ds = create_data_source(&db_type);
    ds.open(&db_path).map_err(|e| {
        log::error!("[DB] 数据库打开失败: {}", e);
        "打开数据库失败，请检查文件路径".to_string()
    })?;
    log::info!("[DB] 数据库打开成功");

    let count = ds.get_record_count()?;
    let date_range = ds.get_date_range()?;
    let providers = ds.get_providers()?;
    let models = ds.get_models()?;
    log::info!("[DB] 记录数={}, 日期范围={:?}, 供应商={}, 模型={}", count, date_range, providers.len(), models.len());

    // 刷新定价引擎
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
        match pricing.refresh(&app_db) {
            Ok(()) => log::info!("[DB] 定价引擎刷新成功, 模型数={}", pricing.size()),
            Err(e) => log::error!("[DB] 定价引擎刷新失败: {}", e),
        }
    }

    Ok(Some(DatabaseInfo {
        path: db_path,
        record_count: count,
        date_range,
        providers,
        models,
    }))
}

/// 校验并规范化数据库文件路径
fn validate_db_path(file_path: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(file_path);

    // 规范化路径，解析 . 和 .. 等相对组件，防止路径遍历
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| format!("路径不存在或无法访问: {} ({})", file_path, e))?;

    // 校验是否为文件（非目录）
    if !canonical.is_file() {
        return Err(format!("路径不是文件: {}", canonical.display()));
    }

    // 校验文件扩展名
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

    Ok(canonical)
}

#[tauri::command]
pub fn load_database(file_path: String, state: State<AppState>) -> Result<DatabaseInfo, String> {
    let canonical_path = validate_db_path(&file_path)?;
    let canonical_str = canonical_path.to_string_lossy().to_string();
    log::info!("[DB] load_database: {}", canonical_str);

    let db_type = detect_db_type(&canonical_str)?;
    log::info!("[DB] 检测到数据库类型: {:?}", db_type);
    let mut ds = state.data_source.write().map_err(|e| e.to_string())?;
    *ds = create_data_source(&db_type);
    ds.open(&canonical_str).map_err(|e| {
        log::error!("[DB] 数据库打开失败: {}", e);
        "打开数据库失败，请检查文件路径".to_string()
    })?;
    log::info!("[DB] 数据库打开成功");

    let count = ds.get_record_count()?;
    let date_range = ds.get_date_range()?;
    let providers = ds.get_providers()?;
    let models = ds.get_models()?;
    log::info!("[DB] 记录数={}, 供应商={}, 模型={}", count, providers.len(), models.len());

    // 刷新定价引擎
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let mut pricing = state.pricing_engine.write().map_err(|e| e.to_string())?;
        match pricing.refresh(&app_db) {
            Ok(()) => log::info!("[DB] 定价引擎刷新成功, 模型数={}", pricing.size()),
            Err(e) => log::error!("[DB] 定价引擎刷新失败: {}", e),
        }
    }

    // 记住选择的数据库路径（使用规范化后的路径）
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        if let Err(e) = app_db.set_setting("last_db_path", &canonical_str) {
            log::error!("[DB] 保存数据库路径失败: {}", e);
        }
    }

    Ok(DatabaseInfo {
        path: canonical_str,
        record_count: count,
        date_range,
        providers,
        models,
    })
}

#[tauri::command]
pub fn refresh_database(state: State<AppState>) -> Result<RefreshResult, String> {
    let ds = state.data_source.read().map_err(|e| e.to_string())?;
    if !ds.is_open() {
        return Ok(RefreshResult {
            has_new: false,
            record_count: None,
        });
    }

    let current_max = ds.get_latest_timestamp();
    let prev_max = *state.db_latest_timestamp.lock().map_err(|e| e.to_string())?;

    if current_max != prev_max {
        *state.db_latest_timestamp.lock().map_err(|e| e.to_string())? = current_max;
        let count = ds.get_record_count()?;
        Ok(RefreshResult {
            has_new: true,
            record_count: Some(count),
        })
    } else {
        Ok(RefreshResult {
            has_new: false,
            record_count: None,
        })
    }
}

#[tauri::command]
pub fn get_filter_options(state: State<AppState>) -> Result<FilterOptions, String> {
    let ds = state.data_source.read().map_err(|e| e.to_string())?;
    if !ds.is_open() {
        return Ok(FilterOptions {
            providers: Vec::new(),
            models: Vec::new(),
            date_range: DateRange { min: 0, max: 0 },
        });
    }
    let providers = ds.get_providers()?;
    let models = ds.get_models()?;
    let date_range = ds.get_date_range()?;
    Ok(FilterOptions {
        providers,
        models,
        date_range,
    })
}
