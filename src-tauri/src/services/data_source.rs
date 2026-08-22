use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OpenFlags};
use std::path::Path;

use crate::models::*;

static SOURCE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// 数据源能力声明：每个数据源显式标记支持的能力，
/// 管道与前端据此分流，取代散落的 None/Err/字符串特判。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCapabilities {
    /// 会话管理：数据进入 sessions 表管理流程（会话 Tab 展示、恢复、
    /// 项目分组），实时 Tab 提供二级会话分组。不支持 ≠ 不取 session_id
    /// （去重指纹、日志关联仍会使用）。
    pub session_management: bool,
    /// 原始数据可提供项目归属信息（cwd / workspace / directory）
    pub project_attribution: bool,
    /// 扫描入库 pricing.db 并支持增量（mtime + 行 offset）
    pub incremental_scan: bool,
}

impl Default for SourceCapabilities {
    fn default() -> Self {
        Self {
            session_management: false,
            project_attribution: false,
            incremental_scan: false,
        }
    }
}

pub trait DataSource: Send + Sync {
    fn open(&mut self, path: &str) -> Result<(), String>;
    fn close(&mut self);
    fn is_open(&self) -> bool;

    fn get_record_count(&self) -> Result<i64, String>;
    fn get_latest_timestamp(&self) -> Option<i64>;
    fn get_providers(&self) -> Result<Vec<Provider>, String>;
    fn get_models(&self) -> Result<Vec<String>, String>;
    fn get_date_range(&self) -> Result<DateRange, String>;

    fn get_summary(&self, params: &FilterParams) -> Result<SummaryData, String>;
    fn get_model_breakdown(&self, params: &FilterParams) -> Result<Vec<ModelBreakdown>, String>;
    fn get_provider_breakdown(&self, params: &FilterParams) -> Result<Vec<ProviderBreakdown>, String>;
    fn get_combined_breakdown(&self, params: &FilterParams) -> Result<Vec<CombinedBreakdownRow>, String>;
    fn get_provider_model_tokens(&self, params: &FilterParams) -> Result<Vec<ProviderModelToken>, String>;
    fn get_daily_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String>;
    fn get_hourly_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String>;
    fn get_session_breakdown(&self, params: &FilterParams) -> Result<Vec<SessionBreakdown>, String>;
    fn get_session_max_context_widths(&self, ids: &[String]) -> Result<HashMap<String, i64>, String>;
    fn get_session_model_tokens(&self, params: &FilterParams) -> Result<Vec<SessionModelToken>, String>;
    fn get_session_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String>;
    fn get_session_request_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionRequestToken>, String>;
    fn get_session_model_tokens_for_ids(&self, params: &FilterParams, session_ids: &[String]) -> Result<Vec<SessionModelToken>, String>;
    fn get_session_timestamps(&self, ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String>;
    fn get_model_context_tier_buckets(&self, params: &FilterParams, thresholds: &[i64]) -> Result<Vec<ModelContextTierBucket>, String>;
    fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String>;
    fn get_recent_request_logs_raw(&self, since: Option<i64>) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64, bool)>, String>;

    /// 按 FilterParams 过滤查询原始请求记录，返回 RawRecord。
    /// 这是去重管道的入口：所有聚合查询应从这里取数据。
    fn get_filtered_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String>;

    /// 流式查询请求记录。逐行通过 callback 发射，避免一次性加载全部数据到内存。
    /// 默认实现委托给 `get_recent_request_logs_raw`。
    /// Tuple 末尾 `is_codex` 标记该记录是否来自 OpenAI Codex 协议（用于会话重映射）。
    fn stream_records(
        &self,
        since: Option<i64>,
        on_record: &mut dyn FnMut((String, String, String, i64, i64, i64, i64, i64, i64, bool)),
    ) -> Result<(), String> {
        for record in self.get_recent_request_logs_raw(since)? {
            on_record(record);
        }
        Ok(())
    }

    /// 数据源能力声明（默认全不支持，各服务按实际覆盖）
    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities::default()
    }

    /// 数据源提供的标题来源标签，用于 UI 展示。
    /// CC-Switch 返回 None（标题由 JSONL 或其他 provider 解析），OpenCode 返回 "opencode"。
    fn title_source_tag(&self) -> Option<&'static str> { None }

    /// 从数据源获取会话标题和项目名。默认不提供。
    /// 返回 HashMap<session_id, (title, project_name)>
    fn get_session_titles_from_provider(
        &self,
        _session_ids: &[String],
    ) -> Option<Result<HashMap<String, (String, String)>, String>> {
        None
    }

    /// Cursor 本机归因 token 统计；非 Cursor 源返回 None。
    fn get_cursor_attribution_stats(
        &self,
    ) -> Option<crate::services::cursor_attribution::AttributionTokenStats> {
        None
    }

    /// 刷新 Cursor 本机 Hook 事件缓存。
    fn refresh_cursor_local_events(&mut self) {}

    /// 本数据源内容文件的最新 mtime（上次 open 时记录），用于跳过无变化的重复 open。
    /// 文件型源（Cursor CSV / Proma 目录）返回 Some；SQL 型源返回 None（实时读取，无需重开）。
    fn content_mtime(&self) -> Option<std::time::SystemTime> {
        None
    }

    /// Cursor CSV 分页预览；非 Cursor 源返回 None。
    fn get_cursor_csv_preview(
        &self,
        _page: usize,
        _page_size: usize,
        _filtered_only: bool,
        _model_filter: Option<&str>,
    ) -> Option<crate::services::cursor_attribution::CursorCsvPreviewPage> {
        None
    }

    /// 设置 / 更新行级归因改判；非 Cursor 源返回 Err。
    fn set_cursor_attribution_override(
        &mut self,
        _row_key: &str,
        _action: crate::services::cursor_attribution::OverrideAction,
        _created_at: i64,
        _model: &str,
    ) -> Result<(), String> {
        Err("当前数据源不支持归因改判".to_string())
    }

    /// 取消行级归因改判；非 Cursor 源返回 Err。
    fn clear_cursor_attribution_override(&mut self, _row_key: &str) -> Result<(), String> {
        Err("当前数据源不支持归因改判".to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbType {
    ExternalDb,
    OpenCode,
    AiProxy,
    Cursor,
    ZCode,
    Proma,
    Dsh,
    Minimax,
}

impl DbType {
    /// 数据源规范名（canonical name）：CCS / OpenCode / AIProxy / Cursor /
    /// ZCode / Proma / DSH / MiniMax。该名称同时用作：
    /// - 持久化 last_db_paths 的 db_type
    /// - 固定型数据源的 provider_id / provider_name
    /// - session_request_logs 入库的 source 列值
    pub fn label(&self) -> &'static str {
        match self {
            DbType::ExternalDb => "CCS",
            DbType::OpenCode => "OpenCode",
            DbType::AiProxy => "AIProxy",
            DbType::Cursor => "Cursor",
            DbType::ZCode => "ZCode",
            DbType::Proma => "Proma",
            DbType::Dsh => "DSH",
            DbType::Minimax => "MiniMax",
        }
    }

    /// 由 label 反查类型（前端明确指定时使用，不依赖表名探测）。
    /// 兼容历史持久化值："CC-Switch" 与 "AI-Proxy"。
    pub fn from_label(label: &str) -> Option<DbType> {
        match label {
            "CCS" | "CC-Switch" => Some(DbType::ExternalDb),
            "OpenCode" => Some(DbType::OpenCode),
            "AIProxy" | "AI-Proxy" => Some(DbType::AiProxy),
            "Cursor" => Some(DbType::Cursor),
            "ZCode" => Some(DbType::ZCode),
            "Proma" => Some(DbType::Proma),
            "DSH" => Some(DbType::Dsh),
            "MiniMax" => Some(DbType::Minimax),
            _ => None,
        }
    }
}

/// last_db_paths 持久化条目：路径 + 类型一起保存，重启后不再靠表名探测
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PersistedSource {
    pub path: String,
    pub db_type: String,
}

pub struct SourceEntry {
    pub id: String,
    pub path: String,
    pub db_type: DbType,
    pub source: Box<dyn DataSource>,
    pub enabled: bool,
}

impl SourceEntry {
    pub fn to_info(&self) -> SourceInfo {
        SourceInfo {
            id: self.id.clone(),
            path: self.path.clone(),
            db_type: self.db_type.label().to_string(),
            record_count: self.source.get_record_count().unwrap_or(0),
            enabled: self.enabled,
            capabilities: self.source.capabilities(),
        }
    }
}

pub fn detect_db_type(path: &str) -> Result<DbType, String> {
    let conn = Connection::open_with_flags(
        Path::new(path),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("无法打开数据库: {}", e))?;

    let has_proxy_logs: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='proxy_request_logs'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if has_proxy_logs {
        return Ok(DbType::ExternalDb);
    }

    let has_model_usage: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='model_usage'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    let has_turn_usage: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='turn_usage'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    // ZCode 特有问题组合（model_usage + turn_usage），优先于 message（ZCode 也含 message 表）
    if has_model_usage && has_turn_usage {
        return Ok(DbType::ZCode);
    }

    let has_message: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='message'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if has_message {
        return Ok(DbType::OpenCode);
    }

    let has_token_stats: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='token_stats'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if has_token_stats {
        return Ok(DbType::AiProxy);
    }

    Err("无法识别数据库类型".to_string())
}

pub fn create_source_entry(path: &str) -> Result<SourceEntry, String> {
    create_source_entry_with_type(path, None)
}

/// 创建数据源条目。
/// `explicit_type` 为 Some 时使用前端明确指定的类型（不依赖表名探测）；
/// None 时回退到 `detect_db_type` 表名探测（auto-load 等场景）。
pub fn create_source_entry_with_type(path: &str, explicit_type: Option<&DbType>) -> Result<SourceEntry, String> {
    let path_obj = Path::new(path);
    if path_obj.is_dir() && super::cursor_csv::detect_cursor_cache(path) {
        let id = SOURCE_ID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string();
        let mut source = Box::new(super::cursor_csv::CursorCsvService::new()) as Box<dyn DataSource>;
        source.open(path)?;
        return Ok(SourceEntry {
            id,
            path: path.to_string(),
            db_type: DbType::Cursor,
            source,
            enabled: true,
        });
    }

    if path_obj.is_dir() && super::proma_dir::detect_proma_dir(path) {
        let id = SOURCE_ID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string();
        let mut source = Box::new(super::proma_dir::PromaDirService::new()) as Box<dyn DataSource>;
        source.open(path)?;
        return Ok(SourceEntry {
            id,
            path: path.to_string(),
            db_type: DbType::Proma,
            source,
            enabled: true,
        });
    }

    let db_type = match explicit_type {
        Some(t) => t.clone(),
        None => detect_db_type(path)?,
    };
    let id = SOURCE_ID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string();
    let mut source = match &db_type {
        DbType::ExternalDb => Box::new(super::external_db::ExternalDbService::new()) as Box<dyn DataSource>,
        DbType::OpenCode => Box::new(super::opencode_db::OpenCodeDbService::new()) as Box<dyn DataSource>,
        DbType::AiProxy => Box::new(super::ai_proxy_db::AiProxyDbService::new()) as Box<dyn DataSource>,
        DbType::ZCode => Box::new(super::zcode_db::ZCodeDbService::new()) as Box<dyn DataSource>,
        DbType::Cursor => return Err("Cursor 数据源需使用缓存目录路径".to_string()),
        DbType::Proma => return Err("Proma 数据源需使用数据目录路径".to_string()),
        DbType::Dsh => Box::new(super::dsh_db::DshDbService::new()) as Box<dyn DataSource>,
        DbType::Minimax => Box::new(super::minimax_db::MinimaxDbService::new()) as Box<dyn DataSource>,
    };
    source.open(path)?;
    Ok(SourceEntry { id, path: path.to_string(), db_type, source, enabled: true })
}

// ========== 合并函数（供 realtime 等仍用 per-source SQL 聚合的查询使用）==========

pub fn merge_realtime_buckets(results: Vec<Vec<RealtimeBucket>>) -> Vec<RealtimeBucket> {
    let mut map: HashMap<i64, RealtimeBucket> = HashMap::new();
    for list in results {
        for b in list {
            map.entry(b.bucket)
                .and_modify(|e| {
                    e.requests += b.requests;
                    e.input_tokens += b.input_tokens;
                    e.output_tokens += b.output_tokens;
                    e.cache_read += b.cache_read;
                    e.cache_creation += b.cache_creation;
                })
                .or_insert(b);
        }
    }
    let mut v: Vec<_> = map.into_values().collect();
    v.sort_by_key(|b| b.bucket);
    v
}

pub fn union_models(results: Vec<Vec<String>>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for list in results {
        for m in list {
            if seen.insert(m.clone()) {
                out.push(m);
            }
        }
    }
    out.sort();
    out
}

pub fn merge_date_range(ranges: Vec<DateRange>) -> DateRange {
    if ranges.is_empty() {
        return DateRange { min: 0, max: 0 };
    }
    DateRange {
        min: ranges.iter().map(|r| r.min).min().unwrap_or(0),
        max: ranges.iter().map(|r| r.max).max().unwrap_or(0),
    }
}
