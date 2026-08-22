use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::services::data_source::{create_source_entry_with_type, create_source_entry, DbType, PersistedSource, SourceEntry};
use crate::services::pipeline::run_streaming_dedup;

// ========== 数据库操作命令 ==========

/// CCS 自动发现开关（settings 键 ccs_auto_discover）。
/// 缺省策略：老用户（存在 last_db_paths 等使用痕迹）保持开启；
/// 全新安装默认关闭——应用定位为通用 token 平台，CCS 作为可选子项配置。
fn ccs_auto_discover_enabled(app_db: &crate::services::app_db::AppDbService) -> bool {
    match app_db.get_setting("ccs_auto_discover").as_deref() {
        Some("on") => true,
        Some("off") => false,
        _ => app_db.get_setting("last_db_paths").is_some(),
    }
}

#[tauri::command]
pub fn get_ccs_auto_discover(state: State<AppState>) -> Result<bool, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    Ok(ccs_auto_discover_enabled(&app_db))
}

#[tauri::command]
pub fn set_ccs_auto_discover(enabled: bool, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let value = if enabled { "on" } else { "off" };
    app_db.set_setting("ccs_auto_discover", value)
}

#[tauri::command]
pub fn auto_load_database(state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let paths_json = app_db.get_setting("last_db_paths");
    drop(app_db);

    // 新格式：路径 + 类型一起持久化，按持久化类型直接打开，不再靠表名探测
    if let Some(json) = &paths_json {
        if let Ok(entries) = serde_json::from_str::<Vec<PersistedSource>>(json) {
            if !entries.is_empty() {
                let existing: Vec<PersistedSource> = entries
                    .into_iter()
                    .filter(|e| std::path::Path::new(&e.path).exists())
                    .collect();
                if existing.is_empty() {
                    log::info!("[DB] 记忆的数据库路径均不存在");
                    return Ok(Vec::new());
                }
                return auto_load_paths(&state, existing);
            }
        }
        // 老格式：纯路径数组，fallback 表名探测
        let paths: Vec<String> = serde_json::from_str(json).unwrap_or_default();
        if !paths.is_empty() {
            let existing: Vec<String> = paths
                .into_iter()
                .filter(|p| std::path::Path::new(p).exists())
                .collect();
            if existing.is_empty() {
                log::info!("[DB] 记忆的数据库路径均不存在");
                return Ok(Vec::new());
            }
            let entries: Vec<PersistedSource> = existing
                .into_iter()
                .map(|path| PersistedSource { path, db_type: String::new() })
                .collect();
            return auto_load_paths(&state, entries);
        }
    }

    // 兼容旧版：尝试 last_db_path（已废弃，由 last_db_paths 替代）
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let single = app_db.get_setting("last_db_path");
    drop(app_db);
    if let Some(p) = single {
        if std::path::Path::new(&p).exists() {
            let entries = vec![PersistedSource { path: p, db_type: String::new() }];
            return auto_load_paths(&state, entries);
        }
    }
    // 自动探测默认路径（代码注册时即明确类型，不依赖表名探测）
    // CCS 需用户在设置中开启"自动发现"后才参与默认注册
    let ccs_discover = state
        .app_db
        .lock()
        .map(|db| ccs_auto_discover_enabled(&db))
        .unwrap_or(false);
    let mut defaults: Vec<PersistedSource> = Vec::new();
    if ccs_discover {
        if let Some(p) = crate::utils::get_default_db_path().ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .filter(|p| std::path::Path::new(p).exists())
        {
            defaults.push(PersistedSource { path: p, db_type: "CCS".to_string() });
        }
    }
    if let Some(p) = crate::utils::get_default_opencode_db_path().ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .filter(|p| std::path::Path::new(p).exists())
    {
        defaults.push(PersistedSource { path: p, db_type: "OpenCode".to_string() });
    }
    if let Some(p) = crate::utils::get_default_ai_proxy_db_path().ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .filter(|p| std::path::Path::new(p).exists())
    {
        defaults.push(PersistedSource { path: p, db_type: "AIProxy".to_string() });
    }
    if cursor_should_auto_load() {
        if let Ok(caches) = crate::utils::list_cursor_account_caches() {
            for acc in caches {
                defaults.push(PersistedSource { path: acc.path.to_string_lossy().to_string(), db_type: "Cursor".to_string() });
            }
        }
    }
    if let Some(p) = crate::utils::get_default_proma_dir().ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .filter(|p| crate::services::proma_dir::detect_proma_dir(p))
    {
        defaults.push(PersistedSource { path: p, db_type: "Proma".to_string() });
    }
    // DSH:当前模式数据目录存在(~/.dsh 或插件 token-usage 目录)则注册(读取路径为应用库 pricing.db,扫描在 auto_load_paths 统一执行)
    {
        let dsh_available = state
            .app_db
            .lock()
            .map(|db| crate::services::dsh_scanner::dsh_source_dir_available(&db))
            .unwrap_or(false);
        if dsh_available {
            if let Ok(p) = crate::utils::get_app_db_path() {
                defaults.push(PersistedSource {
                    path: p.to_string_lossy().to_string(),
                    db_type: "DSH".to_string(),
                });
            }
        }
    }
    // MiniMax Code v2:数据目录存在(~/.minimax/v2/sessions)则注册(读取路径为应用库 pricing.db,扫描在 auto_load_paths 统一执行)
    if crate::services::minimax_scanner::minimax_source_dir_available() {
        if let Ok(p) = crate::utils::get_app_db_path() {
            defaults.push(PersistedSource {
                path: p.to_string_lossy().to_string(),
                db_type: "MiniMax".to_string(),
            });
        }
    }
    if !defaults.is_empty() {
        return auto_load_paths(&state, defaults);
    }
    log::info!("[DB] 无可加载的数据库");
    return Ok(Vec::new());
}

fn auto_load_paths(state: &State<AppState>, entries: Vec<PersistedSource>) -> Result<Vec<SourceInfo>, String> {
    // 先扫描 DSH(若当前模式数据目录存在),保证 DshDbService 打开时已有数据
    if let Ok(app_db) = state.app_db.lock() {
        let _ = crate::services::dsh_scanner::scan_dsh_by_mode(&app_db);
    }
    // 再扫描 MiniMax(若数据目录存在),保证 MinimaxDbService 打开时已有数据
    if let Ok(app_db) = state.app_db.lock() {
        let _ = crate::services::minimax_scanner::scan_minimax(&app_db);
    }

    let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
    sources.clear();

    let cache_root = crate::utils::get_cursor_cache_root()
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    for entry in &entries {
        // 丢弃无效旧根路径（扁平 cursor-cache 且无 usage.csv）
        if let Some(ref root) = cache_root {
            if entry.path == *root || path_equals_ignore_slash(&entry.path, root) {
                let csv = std::path::Path::new(&entry.path).join("usage.csv");
                if !csv.is_file() {
                    log::info!("[DB] 跳过旧 Cursor 根路径: {}", entry.path);
                    continue;
                }
            }
        }
        // 类型驱动打开：持久化条目自带 db_type（canonical 名），直接按类型打开，
        // 不做表名探测（Cursor/Proma 目录型由 create_source_entry_with_type 内部识别）。
        // 仅老格式条目（db_type 为空）回退一次探测，加载后 save_paths 会重写为新格式。
        let explicit = DbType::from_label(&entry.db_type);
        let loaded = match explicit.as_ref() {
            Some(t) => create_source_entry_with_type(&entry.path, Some(t)),
            None => create_source_entry(&entry.path),
        };
        match loaded {
            Ok(loaded) => {
                log::info!("[DB] 自动加载: {} ({})", entry.path, loaded.db_type.label());
                sources.push(loaded);
            }
            Err(e) => log::error!("[DB] 加载失败 {}: {}", entry.path, e),
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
            match crate::services::data_source::create_source_entry_with_type(
                &cache_str,
                Some(&crate::services::data_source::DbType::Cursor),
            ) {
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

    // DSH:若当前模式数据目录存在,补注册数据源(读取路径为应用库 pricing.db;数据已由开头 scan_dsh_by_mode 入库)
    // 持久化的 last_db_paths 不含 DSH,需在此补注册,否则启动时 DSH 源不会加载
    {
        let dsh_available = state
            .app_db
            .lock()
            .map(|db| crate::services::dsh_scanner::dsh_source_dir_available(&db))
            .unwrap_or(false);
        if dsh_available {
            let already = sources
                .iter()
                .any(|s| matches!(s.db_type, crate::services::data_source::DbType::Dsh));
            if !already {
                if let Ok(p) = crate::utils::get_app_db_path() {
                    let path_str = p.to_string_lossy().to_string();
                    match crate::services::data_source::create_source_entry_with_type(
                        &path_str,
                        Some(&crate::services::data_source::DbType::Dsh),
                    ) {
                        Ok(entry) => {
                            log::info!("[DB] 自动加载 DSH 源: {}", path_str);
                            sources.push(entry);
                        }
                        Err(e) => log::error!("[DB] DSH 源加载失败: {}", e),
                    }
                }
            }
        }
    }

    // MiniMax:若数据目录存在,补注册数据源(读取路径为应用库 pricing.db;数据已由开头 scan_minimax 入库)
    if crate::services::minimax_scanner::minimax_source_dir_available() {
        let already = sources
            .iter()
            .any(|s| matches!(s.db_type, crate::services::data_source::DbType::Minimax));
        if !already {
            if let Ok(p) = crate::utils::get_app_db_path() {
                let path_str = p.to_string_lossy().to_string();
                match crate::services::data_source::create_source_entry_with_type(
                    &path_str,
                    Some(&crate::services::data_source::DbType::Minimax),
                ) {
                    Ok(entry) => {
                        log::info!("[DB] 自动加载 MiniMax 源: {}", path_str);
                        sources.push(entry);
                    }
                    Err(e) => log::error!("[DB] MiniMax 源加载失败: {}", e),
                }
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
pub fn load_database(file_path: String, db_type: Option<String>, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let canonical = validate_db_path(&file_path)?;
    let canonical_str = canonical.to_string_lossy().to_string();
    log::info!("[DB] load_database: {} (type={:?})", canonical_str, db_type);

    // 类型驱动：调用方必须显式指定数据源类型，不做表名探测
    let explicit = db_type
        .as_deref()
        .and_then(crate::services::data_source::DbType::from_label)
        .ok_or_else(|| "必须指定有效的数据源类型 (dbType)".to_string())?;
    let entry = create_source_entry_with_type(&canonical_str, Some(&explicit))?;
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
pub fn add_database(file_path: String, db_type: Option<String>, state: State<AppState>) -> Result<Vec<SourceInfo>, String> {
    // Proma 是目录型数据源，走目录校验；其余为 SQLite 文件校验
    let canonical = if db_type.as_deref() == Some("Proma") {
        validate_proma_dir(&file_path)?
    } else {
        validate_db_path(&file_path)?
    };
    let canonical_str = canonical.to_string_lossy().to_string();
    log::info!("[DB] add_database: {} (type={:?})", canonical_str, db_type);

    // 检查是否已加载，避免重复
    {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        if sources.iter().any(|s| s.path == canonical_str) {
            return Err(format!("数据库已加载: {}", canonical_str));
        }
    }

    // 类型驱动：调用方必须显式指定数据源类型，不做表名探测
    let explicit = db_type
        .as_deref()
        .and_then(crate::services::data_source::DbType::from_label)
        .ok_or_else(|| "必须指定有效的数据源类型 (dbType)".to_string())?;
    let entry = create_source_entry_with_type(&canonical_str, Some(&explicit))?;
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

    // MiniMax:增量扫描(若存在 MiniMax 源)。scanner 内部按 mtime 跳过未变文件,开销可接受
    {
        let has_minimax = {
            let sources = state.data_sources.read().map_err(|e| e.to_string())?;
            sources
                .iter()
                .any(|s| matches!(s.db_type, crate::services::data_source::DbType::Minimax))
        };
        if has_minimax {
            if let Ok(app_db) = state.app_db.lock() {
                let _ = crate::services::minimax_scanner::scan_minimax(&app_db);
            }
        }
    }

    // DSH:增量扫描(若存在 DSH 源,按当前模式扫插件数据或会话日志)。
    // scanner 内部按 mtime 跳过未变文件,开销可接受
    let (dsh_plugin_mode, dsh_plugin_dir) = {
        let has_dsh = {
            let sources = state.data_sources.read().map_err(|e| e.to_string())?;
            sources
                .iter()
                .any(|s| matches!(s.db_type, crate::services::data_source::DbType::Dsh))
        };
        if has_dsh {
            if let Ok(app_db) = state.app_db.lock() {
                let use_plugin = crate::services::dsh_scanner::dsh_use_plugin(&app_db);
                let _ = crate::services::dsh_scanner::scan_dsh_by_mode(&app_db);
                let plugin_dir = crate::services::dsh_scanner::resolve_dsh_plugin_dir(&app_db).ok();
                (use_plugin, plugin_dir)
            } else {
                (false, None)
            }
        } else {
            (false, None)
        }
    };

    // Phase 1: 快速检查文件 mtime，无变化则直接返回
    {
        let sources = state.data_sources.read().map_err(|e| e.to_string())?;
        if sources.is_empty() {
            return Ok(RefreshResult { has_new: false, record_count: None });
        }

        let mut mtimes = state.db_file_mtimes.lock().map_err(|e| e.to_string())?;
        let mut any_changed = false;
        for entry in sources.iter() {
            let mtime = source_mtime(&entry.path, &entry.db_type, dsh_plugin_mode, dsh_plugin_dir.as_deref())
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
pub fn scan_dsh_now(state: State<AppState>) -> Result<crate::services::dsh_scanner::DshScanResult, String> {
    // 1. 按当前模式扫描入库(插件数据 / 会话日志)
    let (result, dsh_available) = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let available = crate::services::dsh_scanner::dsh_source_dir_available(&app_db);
        let result = crate::services::dsh_scanner::scan_dsh_by_mode(&app_db)?;
        (result, available)
    };
    // 2. 确保 DSH 源已注册(若当前模式数据目录存在且尚未注册),否则扫描了也无数据源可读
    if dsh_available {
        let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
        let already = sources
            .iter()
            .any(|s| matches!(s.db_type, crate::services::data_source::DbType::Dsh));
        if !already {
            if let Ok(p) = crate::utils::get_app_db_path() {
                let path_str = p.to_string_lossy().to_string();
                match crate::services::data_source::create_source_entry_with_type(
                    &path_str,
                    Some(&crate::services::data_source::DbType::Dsh),
                ) {
                    Ok(entry) => {
                        log::info!("[DB] 注册 DSH 源: {}", path_str);
                        sources.push(entry);
                    }
                    Err(e) => log::error!("[DB] DSH 源注册失败: {}", e),
                }
            }
        }
    }
    Ok(result)
}

#[tauri::command]
pub fn scan_minimax_now(state: State<AppState>) -> Result<crate::services::dsh_scanner::DshScanResult, String> {
    // 1. 扫描 MiniMax 会话日志入库(~/.minimax/v2/sessions,增量)
    let (result, minimax_available) = {
        if crate::services::minimax_scanner::minimax_source_dir_available() {
            let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
            let result = crate::services::minimax_scanner::scan_minimax(&app_db)?;
            (result, true)
        } else {
            // 目录不存在:仍返回现有记录数(与 DSH 目录缺失时行为一致)
            let mut result = crate::services::dsh_scanner::DshScanResult {
                files_scanned: 0,
                imported: 0,
                skipped: 0,
                errors: 0,
                total_records: 0,
            };
            if let Ok(app_db) = state.app_db.lock() {
                result.total_records = app_db
                    .get_session_log_count(crate::services::minimax_scanner::MINIMAX_SOURCE)
                    .unwrap_or(0);
            }
            (result, false)
        }
    };
    // 2. 确保 MiniMax 源已注册(数据目录存在且尚未注册),否则扫描了也无数据源可读
    if minimax_available {
        let mut sources = state.data_sources.write().map_err(|e| e.to_string())?;
        let already = sources
            .iter()
            .any(|s| matches!(s.db_type, crate::services::data_source::DbType::Minimax));
        if !already {
            if let Ok(p) = crate::utils::get_app_db_path() {
                let path_str = p.to_string_lossy().to_string();
                match crate::services::data_source::create_source_entry_with_type(
                    &path_str,
                    Some(&crate::services::data_source::DbType::Minimax),
                ) {
                    Ok(entry) => {
                        log::info!("[DB] 注册 MiniMax 源: {}", path_str);
                        sources.push(entry);
                    }
                    Err(e) => log::error!("[DB] MiniMax 源注册失败: {}", e),
                }
            }
        }
    }
    Ok(result)
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

    // 供应商筛选按数据源粒度：选项为各数据源 canonical 名（CCS / OpenCode / ...），
    // 不再暴露数据源内部的 provider_id（CCS 的 UUID、OpenCode 的 providerID 等）；
    // 聚合查询由 scope_to_provider_source 把命中的 provider_id 解析为数据源级过滤。
    // 同一类型多个源（如多 Cursor 账号）按类型去重。
    let mut seen_types = std::collections::HashSet::new();
    let providers: Vec<Provider> = sources
        .iter()
        .filter(|s| s.enabled)
        .filter(|s| seen_types.insert(s.db_type.label().to_string()))
        .map(|s| Provider {
            id: s.db_type.label().to_string(),
            name: s.db_type.label().to_string(),
        })
        .collect();

    let mut all_models = Vec::new();
    let mut all_ranges = Vec::new();

    for entry in sources.iter().filter(|s| s.enabled) {
        if let Ok(m) = entry.source.get_models() { all_models.push(m); }
        if let Ok(r) = entry.source.get_date_range() { all_ranges.push(r); }
    }

    Ok(FilterOptions {
        providers,
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

/// Proma 目录型数据源校验：必须是目录且能识别为 Proma 数据目录
fn validate_proma_dir(file_path: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(file_path);
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| format!("路径不存在或无法访问: {} ({})", file_path, e))?;
    if !canonical.is_dir() {
        return Err(format!("路径不是目录: {}", canonical.display()));
    }
    if !crate::services::proma_dir::detect_proma_dir(&canonical.to_string_lossy()) {
        return Err(format!(
            "目录不是 Proma 数据目录（缺少 agent-sessions）: {}",
            canonical.display()
        ));
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
    let entries: Vec<PersistedSource> = info.iter().map(|s| PersistedSource {
        path: s.path.clone(),
        db_type: s.db_type.clone(),
    }).collect();
    if let Ok(json) = serde_json::to_string(&entries) {
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
    pub z_code: Option<String>,
    pub proma: Option<String>,
    pub dsh: Option<String>,
    pub minimax: Option<String>,
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
    let z_code = crate::utils::get_default_zcode_db_path().ok()
        .map(|p| best_default_path(&p));
    let proma = crate::utils::get_default_proma_dir().ok()
        .filter(|p| crate::services::proma_dir::detect_proma_dir(&p.to_string_lossy()))
        .map(|p| p.to_string_lossy().to_string());
    let dsh = crate::utils::get_default_dsh_dir().ok()
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().to_string());
    let minimax = crate::utils::get_default_minimax_dir().ok()
        .filter(|p| p.join("v2").join("sessions").is_dir())
        .map(|p| p.to_string_lossy().to_string());
    Ok(DefaultPaths { cc_switch, opencode, ai_proxy, cursor, z_code, proma, dsh, minimax })
}

fn cursor_should_auto_load() -> bool {
    crate::utils::any_cursor_usage_csv_exists()
}

pub(crate) fn source_mtime(
    path: &str,
    db_type: &crate::services::data_source::DbType,
    dsh_use_plugin: bool,
    dsh_plugin_dir: Option<&std::path::Path>,
) -> Option<std::fs::Metadata> {
    use crate::services::data_source::DbType;
    match db_type {
        DbType::Cursor => {
            let csv = std::path::Path::new(path).join("usage.csv");
            std::fs::metadata(csv).ok()
        }
        DbType::Proma => {
            // 内容变化发生在 agent-sessions 下的 jsonl，取最新文件 mtime；
            // 无 jsonl 时回退目录自身 mtime
            let sessions_dir = std::path::Path::new(path).join("agent-sessions");
            let latest_file = std::fs::read_dir(&sessions_dir)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
                .filter_map(|p| {
                    let meta = std::fs::metadata(&p).ok()?;
                    let mtime = meta.modified().ok()?;
                    Some((mtime, p))
                })
                .max_by_key(|(t, _)| *t)
                .map(|(_, p)| p);
            match latest_file {
                Some(p) => std::fs::metadata(p).ok(),
                None => std::fs::metadata(path).ok(),
            }
        }
        DbType::Dsh => {
            // DSH 数据源 path 是 pricing.db;内容变化发生在当前模式的源目录
            // (插件模式:自定义目录或 ~/.dsh/token-usage;会话模式:~/.dsh/sessions),
            // 取最新源文件 mtime;无文件时回退目录 mtime
            if dsh_use_plugin {
                match dsh_plugin_dir {
                    Some(dir) => crate::services::dsh_plugin_scanner::latest_plugin_file_mtime(dir)
                        .or_else(|| std::fs::metadata(dir).ok()),
                    None => std::fs::metadata(path).ok(),
                }
            } else {
                match crate::utils::get_default_dsh_dir() {
                    Ok(dir) => crate::services::dsh_scanner::latest_session_file_mtime(&dir)
                        .or_else(|| std::fs::metadata(&dir).ok()),
                    Err(_) => std::fs::metadata(path).ok(),
                }
            }
        }
        DbType::Minimax => {
            // MiniMax 数据源 path 是 pricing.db;内容变化发生在 ~/.minimax/v2/sessions,
            // 取最新 messages.jsonl mtime;无文件时回退目录 mtime
            match crate::utils::get_default_minimax_dir() {
                Ok(dir) => crate::services::minimax_scanner::latest_session_file_mtime(&dir)
                    .or_else(|| std::fs::metadata(&dir).ok()),
                Err(_) => std::fs::metadata(path).ok(),
            }
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

// ========== DSH 插件数据模式 ==========

/// dsh-token-usage 插件仓库地址(设置面板展示与跳转)
pub const DSH_PLUGIN_REPO_URL: &str = "https://github.com/LaoYueHanNi/dsh-token-usage";

/// DSH 数据源设置(序列化给前端)
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSettings {
    /// 当前是否使用插件数据(关闭时走会话扫描)
    pub use_plugin: bool,
    /// DSH 数据目录(~/.dsh,存在时)
    pub data_dir: Option<String>,
    /// 用户自定义的插件数据目录(未设置为 None,空串视为未设置)
    pub custom_data_dir: Option<String>,
    /// 插件数据目录(实际生效:自定义目录或计算默认路径,可能不存在)
    pub plugin_data_dir: Option<String>,
    /// 插件是否已安装(插件数据目录存在)
    pub plugin_installed: bool,
    /// 插件 usage-*.jsonl 文件数
    pub usage_files: u32,
    /// 已入库的 DSH 记录总数(source='dsh')
    pub total_records: i64,
}

fn collect_dsh_settings(app_db: &crate::services::app_db::AppDbService) -> DshSettings {
    let use_plugin = crate::services::dsh_scanner::dsh_use_plugin(app_db);
    let data_dir = crate::utils::get_default_dsh_dir()
        .ok()
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().to_string());
    let custom_data_dir = app_db
        .get_setting(crate::services::dsh_scanner::SETTING_DSH_PLUGIN_DATA_DIR)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let plugin_data_dir = crate::services::dsh_scanner::resolve_dsh_plugin_dir(app_db)
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    let plugin_installed = plugin_data_dir
        .as_ref()
        .map(|p| std::path::Path::new(p).is_dir())
        .unwrap_or(false);
    let usage_files = plugin_data_dir
        .as_ref()
        .map(|p| crate::services::dsh_plugin_scanner::walk_plugin_files(std::path::Path::new(p)).len() as u32)
        .unwrap_or(0);
    let total_records = app_db.get_session_log_count(crate::services::dsh_scanner::DSH_SOURCE).unwrap_or(0);
    DshSettings {
        use_plugin,
        data_dir,
        custom_data_dir,
        plugin_data_dir,
        plugin_installed,
        usage_files,
        total_records,
    }
}

#[tauri::command]
pub fn dsh_settings(state: State<AppState>) -> Result<DshSettings, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    Ok(collect_dsh_settings(&app_db))
}

#[tauri::command]
pub fn set_dsh_plugin_mode(use_plugin: bool, state: State<AppState>) -> Result<DshSettings, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    crate::services::dsh_scanner::set_dsh_use_plugin(&app_db, use_plugin)?;
    log::info!("[DB] DSH 数据来源切换: {}", if use_plugin { "插件数据" } else { "会话扫描" });
    Ok(collect_dsh_settings(&app_db))
}

/// 设置 dsh-token-usage 插件数据目录。
///
/// - `Some(dir)`:目录必须存在,规范化后写入 settings(不校验目录内容,
///   允许先配目录再装插件)
/// - `None` / 空串:清空设置,恢复默认解析(`$DSH_HOME/token-usage` / `~/.dsh/token-usage`)
///
/// 只写设置不触发扫描,由前端按当前模式决定是否立即扫描(与切换模式的风格一致)。
#[tauri::command]
pub fn set_dsh_plugin_data_dir(
    dir: Option<String>,
    state: State<AppState>,
) -> Result<DshSettings, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    let normalized = match dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => {
            let canonical = canonicalize_dir(d)?;
            log::info!("[DB] DSH 插件数据目录自定义: {}", canonical.display());
            canonical.to_string_lossy().to_string()
        }
        None => {
            log::info!("[DB] DSH 插件数据目录恢复默认");
            String::new()
        }
    };
    app_db.set_setting(crate::services::dsh_scanner::SETTING_DSH_PLUGIN_DATA_DIR, &normalized)?;
    Ok(collect_dsh_settings(&app_db))
}

/// 目录校验:canonicalize 确保存在且为目录,并去掉 Windows `\\?\` 前缀
fn canonicalize_dir(dir: &str) -> Result<std::path::PathBuf, String> {
    let canonical = std::fs::canonicalize(dir)
        .map_err(|e| format!("目录不存在或无法访问: {} ({})", dir, e))?;
    if !canonical.is_dir() {
        return Err(format!("路径不是目录: {}", canonical.display()));
    }
    #[cfg(target_os = "windows")]
    {
        let s = canonical.to_string_lossy().to_string();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(std::path::PathBuf::from(stripped));
        }
    }
    Ok(canonical)
}

#[tauri::command]
pub fn open_plugin_repo() -> Result<(), String> {
    crate::utils::open_url_in_browser(DSH_PLUGIN_REPO_URL)
}
