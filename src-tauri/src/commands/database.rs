use tauri::State;

use crate::AppState;
use crate::models::*;

// ========== 数据库操作命令 ==========

#[tauri::command]
pub fn select_database(state: State<AppState>) -> Result<Option<DatabaseInfo>, String> {
    // Tauri 的文件对话框由前端 @tauri-apps/plugin-dialog 处理
    // 此命令由前端调用 open() 拿到路径后调用 load_database
    Err("请使用前端对话框选择文件后调用 load_database".to_string())
}

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
            let default = crate::utils::get_default_db_path();
            default.to_str().unwrap().to_string()
        });
    drop(app_db);

    eprintln!("[DB] auto_load_database: path={}", db_path);
    if !std::path::Path::new(&db_path).exists() {
        eprintln!("[DB] 数据库不存在");
        return Ok(None);
    }

    let mut ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    ext_db.open(&db_path)?;
    eprintln!("[DB] 数据库打开成功");

    let count = ext_db.get_record_count()?;
    let date_range = ext_db.get_date_range()?;
    let providers = ext_db.get_providers()?;
    let models = ext_db.get_models()?;
    eprintln!("[DB] 记录数={}, 日期范围={:?}, 供应商={}, 模型={}", count, date_range, providers.len(), models.len());

    // 刷新定价引擎
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let mut pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;
        match pricing.refresh(&ext_db, &app_db) {
            Ok(()) => eprintln!("[DB] 定价引擎刷新成功, 模型数={}", pricing.size()),
            Err(e) => eprintln!("[DB] 定价引擎刷新失败: {}", e),
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

#[tauri::command]
pub fn load_database(file_path: String, state: State<AppState>) -> Result<DatabaseInfo, String> {
    eprintln!("[DB] load_database: {}", file_path);
    let mut ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    ext_db.open(&file_path)?;
    eprintln!("[DB] 数据库打开成功");

    let count = ext_db.get_record_count()?;
    let date_range = ext_db.get_date_range()?;
    let providers = ext_db.get_providers()?;
    let models = ext_db.get_models()?;
    eprintln!("[DB] 记录数={}, 供应商={}, 模型={}", count, providers.len(), models.len());

    // 刷新定价引擎
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let mut pricing = state.pricing_engine.lock().map_err(|e| e.to_string())?;
        match pricing.refresh(&ext_db, &app_db) {
            Ok(()) => eprintln!("[DB] 定价引擎刷新成功, 模型数={}", pricing.size()),
            Err(e) => eprintln!("[DB] 定价引擎刷新失败: {}", e),
        }
    }

    // 记住选择的数据库路径
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        if let Err(e) = app_db.set_setting("last_db_path", &file_path) {
            eprintln!("[DB] 保存数据库路径失败: {}", e);
        }
    }

    Ok(DatabaseInfo {
        path: file_path,
        record_count: count,
        date_range,
        providers,
        models,
    })
}

#[tauri::command]
pub fn refresh_database(state: State<AppState>) -> Result<RefreshResult, String> {
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    if !ext_db.is_open() {
        return Ok(RefreshResult {
            has_new: false,
            record_count: None,
        });
    }

    let current_max = ext_db.get_latest_timestamp();
    let prev_max = *state.db_latest_timestamp.lock().map_err(|e| e.to_string())?;

    if current_max != prev_max {
        *state.db_latest_timestamp.lock().map_err(|e| e.to_string())? = current_max;
        let count = ext_db.get_record_count()?;
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
    let ext_db = state.external_db.lock().map_err(|e| e.to_string())?;
    if !ext_db.is_open() {
        return Ok(FilterOptions {
            providers: Vec::new(),
            models: Vec::new(),
            date_range: DateRange { min: 0, max: 0 },
        });
    }
    let providers = ext_db.get_providers()?;
    let models = ext_db.get_models()?;
    let date_range = ext_db.get_date_range()?;
    Ok(FilterOptions {
        providers,
        models,
        date_range,
    })
}
