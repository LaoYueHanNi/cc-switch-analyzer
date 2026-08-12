// 工具函数

/// 云端定价文件 URL（Gitee raw 文件地址）
pub const CLOUD_PRICING_URL: &str = "https://gitee.com/oyw125/model-price-table/raw/master/model_pricing.json";

/// 会话分析 Top N
pub const SESSION_TOP_N: i64 = 500;

/// 实时监控窗口（秒）
pub const REALTIME_WINDOW_SEC: i64 = 3600;

/// TrafficMonitor 插件 API 默认端口
pub const TM_API_PORT: u16 = 19810;
/// TrafficMonitor 插件 API 端口搜索上限
pub const TM_API_PORT_MAX: u16 = 19820;
/// TrafficMonitor 插件 API 缓存 TTL（秒）
pub const TM_CACHE_TTL_SECS: u64 = 30;

/// YYYY-MM-DD 字符串转 Unix 秒，解析失败返回 None
pub fn date_str_to_epoch(date_str: &str) -> Option<i64> {
    let parts: Vec<i64> = date_str
        .split('-')
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() != 3 {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(parts[0] as i32, parts[1] as u32, parts[2] as u32)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
}

/// 获取应用数据库路径，HOME 目录不可用时返回错误
pub fn get_app_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    let dir = home.join(".cc-switch-analyzer");
    std::fs::create_dir_all(&dir).ok();
    Ok(dir.join("pricing.db"))
}

/// 获取默认外部数据库路径，HOME 目录不可用时返回错误
pub fn get_default_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".cc-switch").join("cc-switch.db"))
}

/// 获取默认 OpenCode 数据库路径
pub fn get_default_opencode_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".local").join("share").join("opencode").join("opencode.db"))
}

/// 获取默认 AI Proxy 数据库路径
pub fn get_default_ai_proxy_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".ai-agent-tools").join("data").join("access_log.db"))
}

/// 获取默认 ZCode 数据库路径
pub fn get_default_zcode_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".zcode").join("cli").join("db").join("db.sqlite"))
}

/// Cursor 凭证文件路径
pub fn get_cursor_credentials_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".cc-switch-analyzer").join("cursor-credentials.json"))
}

/// Cursor 用量 CSV 缓存根目录（其下按 userId 分子目录）
pub fn get_cursor_cache_root() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".cc-switch-analyzer").join("cursor-cache"))
}

/// 兼容旧名：等同于 [`get_cursor_cache_root`]
pub fn get_cursor_cache_dir() -> Result<std::path::PathBuf, String> {
    get_cursor_cache_root()
}

/// 将 Cursor userId 净化为安全目录名（仅保留 `[A-Za-z0-9_-]`）
pub fn sanitize_user_id(user_id: &str) -> String {
    let s: String = user_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "_unknown".to_string()
    } else {
        s
    }
}

/// 无 userId 时用 token 的稳定 FNV-1a 目录名（16 hex）
pub fn token_fallback_dir_id(token: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in token.trim().as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

/// 某账号的缓存目录 `cursor-cache/<sanitizedUserId>/`
pub fn get_cursor_account_cache_dir(user_id: &str) -> Result<std::path::PathBuf, String> {
    Ok(get_cursor_cache_root()?.join(sanitize_user_id(user_id)))
}

/// 磁盘上的一个 Cursor 账号缓存
#[derive(Debug, Clone)]
pub struct CursorAccountCache {
    pub user_id: String,
    pub path: std::path::PathBuf,
}

fn read_account_user_id(dir: &std::path::Path) -> Option<String> {
    let meta_path = dir.join("account.json");
    let content = std::fs::read_to_string(meta_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("userId")
        .or_else(|| v.get("user_id"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// 从账号缓存目录解析 userId（account.json → 目录名）
pub fn resolve_account_user_id(dir: &std::path::Path) -> String {
    if let Some(id) = read_account_user_id(dir) {
        return id;
    }
    dir.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("_unknown")
        .to_string()
}

const LEGACY_DIR_NAME: &str = "_legacy";

fn move_file_if_absent(src: &std::path::Path, dst: &std::path::Path) {
    if !src.is_file() {
        return;
    }
    if dst.exists() {
        return;
    }
    if let Some(parent) = dst.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(src, dst).is_err() {
        if std::fs::copy(src, dst).is_ok() {
            let _ = std::fs::remove_file(src);
        }
    }
}

/// 将根目录遗留的扁平 `usage.csv` / overrides 迁入目标账号目录。
/// `target_user_id` 为 None 时迁入 `_legacy`。
pub fn migrate_legacy_cursor_cache(target_user_id: Option<&str>) -> Result<(), String> {
    migrate_legacy_cursor_cache_in(&get_cursor_cache_root()?, target_user_id)
}

/// 可测入口：对指定 root 执行遗留迁移
pub fn migrate_legacy_cursor_cache_in(
    root: &std::path::Path,
    target_user_id: Option<&str>,
) -> Result<(), String> {
    let legacy_csv = root.join("usage.csv");
    let legacy_ov = root.join("attribution-overrides.json");
    if !legacy_csv.is_file() && !legacy_ov.is_file() {
        return Ok(());
    }

    let dir_name = match target_user_id {
        Some(uid) if !uid.trim().is_empty() => sanitize_user_id(uid),
        _ => LEGACY_DIR_NAME.to_string(),
    };
    let dest_dir = root.join(&dir_name);
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("创建账号缓存目录失败: {}", e))?;

    move_file_if_absent(&legacy_csv, &dest_dir.join("usage.csv"));
    move_file_if_absent(&legacy_ov, &dest_dir.join("attribution-overrides.json"));

    let meta_path = dest_dir.join("account.json");
    if !meta_path.exists() {
        let uid = target_user_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| dir_name.clone());
        let meta = serde_json::json!({
            "userId": uid,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        });
        if let Ok(json) = serde_json::to_string_pretty(&meta) {
            let _ = std::fs::write(&meta_path, json);
        }
    }

    // 若首次登录且 _legacy 存在、目标账号目录尚无完整数据，尝试归并
    if let Some(uid) = target_user_id {
        let legacy_dir = root.join(LEGACY_DIR_NAME);
        if legacy_dir.is_dir() && sanitize_user_id(uid) != LEGACY_DIR_NAME {
            let account_dir = root.join(sanitize_user_id(uid));
            std::fs::create_dir_all(&account_dir).ok();
            move_file_if_absent(
                &legacy_dir.join("usage.csv"),
                &account_dir.join("usage.csv"),
            );
            move_file_if_absent(
                &legacy_dir.join("attribution-overrides.json"),
                &account_dir.join("attribution-overrides.json"),
            );
            let _ = std::fs::remove_dir(&legacy_dir);
        }
    }

    Ok(())
}

/// 扫描 `cursor-cache/*/usage.csv`，返回各账号缓存（先执行遗留迁移）
pub fn list_cursor_account_caches() -> Result<Vec<CursorAccountCache>, String> {
    let root = get_cursor_cache_root()?;
    let _ = migrate_legacy_cursor_cache_in(&root, None);
    list_cursor_account_caches_in(&root)
}

/// 可测入口：列出指定 root 下的账号缓存
pub fn list_cursor_account_caches_in(root: &std::path::Path) -> Result<Vec<CursorAccountCache>, String> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let entries = std::fs::read_dir(root).map_err(|e| format!("读取 Cursor 缓存目录失败: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let csv = path.join("usage.csv");
        if !csv.is_file() {
            continue;
        }
        let user_id = resolve_account_user_id(&path);
        out.push(CursorAccountCache { user_id, path });
    }
    out.sort_by(|a, b| a.user_id.cmp(&b.user_id));
    Ok(out)
}

/// 是否存在任意账号的 usage.csv
pub fn any_cursor_usage_csv_exists() -> bool {
    list_cursor_account_caches()
        .map(|list| !list.is_empty())
        .unwrap_or(false)
}

/// Cursor Hook requests.jsonl 备份目录
pub fn get_hook_backup_dir() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".cc-switch-analyzer").join("hook-backups"))
}

/// 任一已有账号的 usage.csv（存在性检查用）；无账号时回落根目录旧路径
#[allow(dead_code)]
pub fn get_cursor_usage_csv_path() -> Result<std::path::PathBuf, String> {
    if let Ok(list) = list_cursor_account_caches() {
        if let Some(first) = list.first() {
            return Ok(first.path.join("usage.csv"));
        }
    }
    Ok(get_cursor_cache_root()?.join("usage.csv"))
}

/// Cursor 本机 Hook 日志目录（~/.cursor/local-usage）
pub fn get_cursor_local_usage_dir() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".cursor").join("local-usage"))
}

/// Cursor 用户级 hooks.json 路径
pub fn get_cursor_hooks_json_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 HOME 目录".to_string())?;
    Ok(home.join(".cursor").join("hooks.json"))
}

/// 当前 Unix 秒
pub fn now_epoch_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_str_to_epoch() {
        assert_eq!(date_str_to_epoch("1970-01-01"), Some(0));
        assert_eq!(date_str_to_epoch("2024-01-01"), Some(1704067200));
        assert_eq!(date_str_to_epoch("2024-06-15"), Some(1718409600));
    }

    #[test]
    fn test_date_str_to_epoch_invalid() {
        assert_eq!(date_str_to_epoch(""), None);
        assert_eq!(date_str_to_epoch("invalid"), None);
        assert_eq!(date_str_to_epoch("2024"), None);
        assert_eq!(date_str_to_epoch("2024-13-01"), None);
    }

    #[test]
    fn test_roundtrip() {
        let epoch = date_str_to_epoch("2024-06-15").unwrap();
        let date_str = {
            let dt = chrono::DateTime::from_timestamp(epoch, 0).unwrap();
            dt.format("%Y-%m-%d").to_string()
        };
        assert_eq!(date_str, "2024-06-15");
    }

    #[test]
    fn test_sanitize_user_id() {
        assert_eq!(sanitize_user_id("user_123"), "user_123");
        assert_eq!(sanitize_user_id("a/b\\c"), "a_b_c");
        assert_eq!(sanitize_user_id(""), "_unknown");
    }

    #[test]
    fn test_token_fallback_stable() {
        let a = token_fallback_dir_id("abc::token");
        let b = token_fallback_dir_id("abc::token");
        let c = token_fallback_dir_id("other::token");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn test_migrate_legacy_flat_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("usage.csv"), "Date,Model\nrow\n").unwrap();
        std::fs::write(
            root.join("attribution-overrides.json"),
            "{\"version\":1,\"overrides\":{}}",
        )
        .unwrap();

        migrate_legacy_cursor_cache_in(root, Some("userA")).unwrap();

        assert!(!root.join("usage.csv").exists());
        assert!(root.join("userA").join("usage.csv").is_file());
        assert!(root.join("userA").join("attribution-overrides.json").is_file());
        assert!(root.join("userA").join("account.json").is_file());

        let list = list_cursor_account_caches_in(root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].user_id, "userA");
    }

    #[test]
    fn test_migrate_does_not_overwrite_existing_account_csv() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let account = root.join("userA");
        std::fs::create_dir_all(&account).unwrap();
        std::fs::write(account.join("usage.csv"), "Date,Model\nKEEP\n").unwrap();
        std::fs::write(root.join("usage.csv"), "Date,Model\nLEGACY\n").unwrap();

        migrate_legacy_cursor_cache_in(root, Some("userA")).unwrap();

        let content = std::fs::read_to_string(account.join("usage.csv")).unwrap();
        assert!(content.contains("KEEP"));
        assert!(!content.contains("LEGACY"));
        // 根目录遗留未被移走（目标已存在）
        assert!(root.join("usage.csv").is_file());
    }

    #[test]
    fn test_list_two_accounts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for uid in ["acc1", "acc2"] {
            let d = root.join(uid);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("usage.csv"), "Date,Model\n").unwrap();
            std::fs::write(
                d.join("account.json"),
                format!("{{\"userId\":\"{}\"}}", uid),
            )
            .unwrap();
        }
        let list = list_cursor_account_caches_in(root).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].user_id, "acc1");
        assert_eq!(list[1].user_id, "acc2");
    }
}
