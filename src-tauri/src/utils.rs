// 工具函数

/// 云端定价文件 URL（Gitee raw 文件地址）
pub const CLOUD_PRICING_URL: &str = "https://gitee.com/oyw125/model-price-table/raw/master/model_pricing.json";

/// 缓存窗口历史范围（天）
pub const CACHE_WINDOW_DAYS: i64 = 30;

/// 会话分析 Top N
pub const SESSION_TOP_N: i64 = 50;

/// 实时监控窗口（秒）
pub const REALTIME_WINDOW_SEC: i64 = 3600;

/// YYYY-MM-DD 字符串转 Unix 秒
pub fn date_str_to_epoch(date_str: &str) -> i64 {
    let parts: Vec<i64> = date_str
        .split('-')
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() != 3 {
        return 0;
    }
    chrono::NaiveDate::from_ymd_opt(parts[0] as i32, parts[1] as u32, parts[2] as u32)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0)
}

/// Date 对象（YYYY-MM-DD）转当天 00:00:00 UTC 的 Unix 秒
pub fn to_epoch_seconds(date_str: &str) -> i64 {
    date_str_to_epoch(date_str)
}

/// Date 对象转次日凌晨的 Unix 秒（exclusive end）
pub fn to_exclusive_end_epoch(date_str: &str) -> i64 {
    let epoch = date_str_to_epoch(date_str);
    if epoch == 0 {
        return 0;
    }
    epoch + 86400
}

/// 获取应用数据库路径
pub fn get_app_db_path() -> std::path::PathBuf {
    let home = dirs::home_dir().expect("无法获取 HOME 目录");
    let dir = home.join(".cc-switch-analyzer");
    std::fs::create_dir_all(&dir).ok();
    dir.join("pricing.db")
}

/// 获取默认外部数据库路径
pub fn get_default_db_path() -> std::path::PathBuf {
    let home = dirs::home_dir().expect("无法获取 HOME 目录");
    home.join(".cc-switch").join("cc-switch.db")
}

/// 当前 Unix 秒
pub fn now_epoch_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}
