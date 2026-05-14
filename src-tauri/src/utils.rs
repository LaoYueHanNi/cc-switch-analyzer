// 工具函数

/// 云端定价文件 URL（Gitee raw 文件地址）
pub const CLOUD_PRICING_URL: &str = "https://gitee.com/oyw125/model-price-table/raw/master/model_pricing.json";

/// 会话分析 Top N
pub const SESSION_TOP_N: i64 = 50;

/// 实时监控窗口（秒）
pub const REALTIME_WINDOW_SEC: i64 = 3600;

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
}
