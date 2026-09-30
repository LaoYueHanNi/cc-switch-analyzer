//! 今日用量数据的共享查询管道。
//!
//! 供两处消费：Windows 的 TrafficMonitor HTTP 服务（`http_server.rs`）
//! 与 macOS 的菜单栏常驻显示（`menubar.rs`）。
//! 复用前端查询管道（`compute_precompute`），使用独立的 `DataSource`
//! 实例避免与前端的 `rusqlite::Connection` 并发冲突。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Serialize;

use crate::commands::database::source_mtime;
use crate::commands::query::compute_precompute;
use crate::models::FilterParams;
use crate::services::data_source::{create_source_entry_with_type, DbType, SourceEntry};
use crate::SharedState;
use crate::utils::*;

/// 今日用量汇总（HTTP JSON 响应结构 / 菜单栏格式化输入）
#[derive(Clone, Serialize)]
pub struct TodayData {
    #[serde(rename = "totalTokens")]
    pub total_tokens: i64,
    #[serde(rename = "inputTokens")]
    pub input_tokens: i64,
    #[serde(rename = "outputTokens")]
    pub output_tokens: i64,
    #[serde(rename = "cacheReadTokens")]
    pub cache_read_tokens: i64,
    #[serde(rename = "cacheCreationTokens")]
    pub cache_creation_tokens: i64,
    #[serde(rename = "totalCost")]
    pub total_cost: String,
    #[serde(rename = "requestCount")]
    pub request_count: i64,
    /// 未格式化的费用数值（菜单栏等消费方自行格式化），不出现在 JSON 中
    #[serde(skip)]
    pub total_cost_value: f64,
}

/// 独立 DataSource 实例缓存：避免每次查询都重建所有源
/// （重建会重解析 Cursor CSV、重读本机 Hook 日志、重解析 Proma 目录）。
/// 仅在源集合（路径+类型）或文件 mtime 变化时才重建，否则复用长驻实例。
pub struct TodaySourceCache {
    /// 长驻的独立只读数据源实例
    sources: Vec<SourceEntry>,
    /// 上次构建所用的 (路径, 类型) 列表
    signature: Vec<(String, DbType)>,
    /// path → 上次构建时观测到的内容 mtime（None 表示无法获取）
    mtimes: HashMap<String, Option<SystemTime>>,
}

impl Default for TodaySourceCache {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            signature: Vec::new(),
            mtimes: HashMap::new(),
        }
    }
}

/// 查询今日（按 tz_offset 时区的自然日）用量汇总。
/// 消费方各自持有一份 `TodaySourceCache`（互不共享锁，避免相互阻塞）。
pub fn query_today_data(
    shared: &SharedState,
    tz_offset: i64,
    source_cache: &Mutex<TodaySourceCache>,
) -> Result<TodayData, String> {
    let now = now_epoch_seconds();
    let local_now = now + tz_offset * 3600;
    let today_start_local = local_now - (local_now % 86400);
    let from_epoch = today_start_local - tz_offset * 3600;
    let to_epoch = from_epoch + 86400;

    let params = FilterParams {
        from_epoch: Some(from_epoch),
        to_epoch: Some(to_epoch),
        tz_offset: Some(tz_offset),
        provider_id: None,
        model_id: None,
        ccs_filter_session_apps: None,
    };

    // 只在锁期间读取 (路径, 类型) 列表，释放后用于 staleness 判断
    // 按 enabled 过滤：查询需尊重用户在 UI 里的数据源开关
    // 必须带上 db_type：DSH 源的 path 是应用库 pricing.db，没有
    // proxy_request_logs 等表，靠 detect_db_type 表名探测会失败而被丢弃
    let entries: Vec<(String, DbType)> = {
        let sources = shared.data_sources.read().map_err(|e| e.to_string())?;
        sources
            .iter()
            .filter(|s| s.enabled)
            .map(|s| (s.path.clone(), s.db_type.clone()))
            .collect()
    };
    if entries.is_empty() {
        return Err("no_database_loaded".to_string());
    }

    // DSH mtime 计算需知道当前模式；本模块无 app_db 句柄，按默认插件目录是否存在推断
    // (用户自定义插件目录时可能推断不准)。
    // 即便推断不准也不影响数据新鲜度——DSH 源每次都实时读取 pricing.db。
    let dsh_plugin_dir = crate::utils::get_default_dsh_plugin_dir().ok();
    let dsh_use_plugin = dsh_plugin_dir
        .as_ref()
        .map(|d| d.is_dir())
        .unwrap_or(false);

    // 观测各源当前内容 mtime（开销远低于重解析）
    let mtimes_now: HashMap<String, Option<SystemTime>> = entries
        .iter()
        .map(|(p, t)| {
            (
                p.clone(),
                source_mtime(p, t, dsh_use_plugin, dsh_plugin_dir.as_deref())
                    .and_then(|m| m.modified().ok()),
            )
        })
        .collect();

    let pricing = shared.pricing_engine.read().map_err(|e| e.to_string())?;

    // 仅在源集合或 mtime 变化时重建独立 DataSource；否则复用长驻实例。
    // 注意：compute_precompute 在持锁期间执行，消费方需为单线程顺序消费，
    // 或保证同一份 source_cache 不被并发查询。
    let result = {
        let mut sc = source_cache.lock().map_err(|e| e.to_string())?;
        let signature_changed = sc.signature != entries;
        let mtime_changed = sc.mtimes != mtimes_now;
        if sc.sources.is_empty() || signature_changed || mtime_changed {
            let rebuilt: Vec<SourceEntry> = entries
                .iter()
                .filter_map(|(p, t)| create_source_entry_with_type(p, Some(t)).ok())
                .collect();
            if rebuilt.is_empty() {
                return Err("no_database_loaded".to_string());
            }
            sc.sources = rebuilt;
            sc.signature = entries;
            sc.mtimes = mtimes_now;
        }
        compute_precompute(&sc.sources, &pricing, &params)?
    };

    let summary = result.summary;

    let total_cost: f64 = result.precomputed.model_costs.values().sum();

    Ok(TodayData {
        total_tokens: summary.total_input + summary.total_output + summary.total_cache_read + summary.total_cache_creation,
        input_tokens: summary.total_input,
        output_tokens: summary.total_output,
        cache_read_tokens: summary.total_cache_read,
        cache_creation_tokens: summary.total_cache_creation,
        total_cost: format!("{:.2}¥", (total_cost * 100.0).round() / 100.0),
        request_count: summary.total_requests,
        total_cost_value: total_cost,
    })
}
