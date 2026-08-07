use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::RwLock;

use serde::Deserialize;

use crate::models::*;
use crate::services::data_source::DataSource;
use crate::services::pipeline::{
    aggregate_combined_records, aggregate_daily_trend, aggregate_hourly_trend,
    aggregate_model_breakdown, aggregate_model_context_tier_buckets, aggregate_provider_breakdown,
    aggregate_provider_model_tokens, aggregate_session_breakdown, aggregate_session_model_tokens,
    aggregate_summary,
};
use crate::utils::{self, SESSION_TOP_N, REALTIME_WINDOW_SEC};

/// Cursor-BYOK 固定 provider 标识（所有调用均归属此提供商）
const PROVIDER_ID: &str = "cursor-byok";
const PROVIDER_NAME: &str = "Cursor-BYOK";
const USAGE_FILE_NAME: &str = "usage.json";

// ========== usage.json（schema v2）结构，容忍缺字段 ==========

#[derive(Deserialize)]
struct UsageFile {
    #[serde(default)]
    recent_events: Vec<UsageEvent>,
}

#[derive(Deserialize)]
struct UsageEvent {
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    kind: String,
    /// UTC ISO8601，如 2026-08-07T03:23:12.6613651Z
    #[serde(default)]
    at: String,
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_read_tokens: i64,
    #[serde(default)]
    cache_write_tokens: i64,
    #[serde(default)]
    usage_present: bool,
}

// ========== context.json / state.json 提取结构 ==========

#[derive(Deserialize)]
struct ContextFile {
    #[serde(default)]
    items: Vec<ContextItem>,
}

#[derive(Deserialize)]
struct ContextItem {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    request_id: String,
    payload: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct StateFile {
    #[serde(default)]
    latest_request_prefix: Option<StateCallInfo>,
    #[serde(default)]
    last_provider_call: Option<StateCallInfo>,
}

#[derive(Deserialize)]
struct StateCallInfo {
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    model: String,
}

/// Cursor-BYOK 数据源：读取 history 目录（usage.json + 会话 context.json/state.json）。
/// 仅做文件读取，不写任何数据。
pub struct CursorByokDbService {
    history_dir: String,
    records: RwLock<Vec<RawRecord>>,
    latest_timestamp: Option<i64>,
    /// 最近一次成功加载时 usage.json 的修改时间，用于跳过无变化重载
    loaded_mtime: Option<std::time::SystemTime>,
    /// request_id → (conversation_id, model)，来自 context.json 的 run_request 项
    request_index: HashMap<String, (String, String)>,
}

impl CursorByokDbService {
    pub fn new() -> Self {
        Self {
            history_dir: String::new(),
            records: RwLock::new(Vec::new()),
            latest_timestamp: None,
            loaded_mtime: None,
            request_index: HashMap::new(),
        }
    }

    pub fn open(&mut self, dir_path: &str) -> Result<(), String> {
        let usage_path = Path::new(dir_path).join(USAGE_FILE_NAME);
        let mtime = std::fs::metadata(&usage_path)
            .map_err(|e| format!("无法读取 usage.json: {} ({})", usage_path.display(), e))?
            .modified()
            .ok();
        // usage.json 未变化且已加载时跳过重解析（查询前会频繁调用 open）
        if mtime == self.loaded_mtime
            && !self
                .records
                .read()
                .map_err(|e| e.to_string())?
                .is_empty()
        {
            return Ok(());
        }

        // 扫描会话目录：context.json 提供 request_id → (conversation_id, model)，
        // state.json 兜底 request_id → model（仅含每个会话最近一次调用）
        let mut index: HashMap<String, (String, String)> = HashMap::new();
        let mut fallback: HashMap<String, String> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(dir_path) {
            for entry in entries.flatten() {
                let conv_dir = entry.path();
                if !conv_dir.is_dir() {
                    continue;
                }
                let conv_id = entry.file_name().to_string_lossy().to_string();
                Self::index_context_file(&conv_dir, &conv_id, &mut index);
                Self::index_state_file(&conv_dir, &mut fallback);
            }
        }

        let content = std::fs::read_to_string(&usage_path)
            .map_err(|e| format!("读取 usage.json 失败: {} ({})", usage_path.display(), e))?;
        let usage: UsageFile = serde_json::from_str(&content)
            .map_err(|e| format!("解析 usage.json 失败: {}", e))?;

        let mut records = Vec::new();
        for event in usage.recent_events {
            // 仅 provider_call 且带有效 usage 的事件映射为请求记录；
            // turn_finalized 是轮次聚合，映射会与 provider_call 重复计数
            if event.kind != "provider_call" || !event.usage_present {
                continue;
            }
            let request_id = event
                .event_id
                .split("::")
                .next()
                .unwrap_or("")
                .to_string();
            let (conv, model) = match index.get(&request_id) {
                Some((c, m)) => (c.clone(), m.clone()),
                None => {
                    let model = fallback
                        .get(&request_id)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    (format!("unknown-{}", request_id), model)
                }
            };
            let created_at = parse_iso_to_epoch(&event.at);
            if created_at <= 0 {
                continue;
            }
            records.push(RawRecord {
                session_id: format!("byok-{}", conv),
                model,
                provider_id: PROVIDER_ID.to_string(),
                created_at,
                input_tokens: event.input_tokens.max(0),
                output_tokens: event.output_tokens.max(0),
                cache_read: event.cache_read_tokens.max(0),
                cache_creation: event.cache_write_tokens.max(0),
                latency: 0,
                is_codex: false,
            });
        }

        self.latest_timestamp = records.iter().map(|r| r.created_at).max();
        self.request_index = index;
        *self.records.write().map_err(|e| e.to_string())? = records;
        self.history_dir = dir_path.to_string();
        self.loaded_mtime = mtime;
        log::info!(
            "[BYOK] 加载完成: {} 条记录, {} 个请求索引",
            self.records.read().map_err(|e| e.to_string())?.len(),
            self.request_index.len()
        );
        Ok(())
    }

    pub fn close(&mut self) {
        self.history_dir.clear();
        if let Ok(mut guard) = self.records.write() {
            guard.clear();
        }
        self.latest_timestamp = None;
        self.loaded_mtime = None;
        self.request_index.clear();
    }

    pub fn is_open(&self) -> bool {
        !self.history_dir.is_empty()
    }

    fn records_read(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<RawRecord>>, String> {
        self.records.read().map_err(|e| format!("数据锁失败: {}", e))
    }

    fn filter_records(&self, params: &FilterParams) -> Vec<RawRecord> {
        let records = match self.records_read() {
            Ok(guard) => guard,
            Err(e) => {
                log::warn!("[BYOK] 读取记录失败: {}", e);
                return Vec::new();
            }
        };
        records
            .iter()
            .filter(|r| record_matches_params(r, params))
            .cloned()
            .collect()
    }

    fn tz_offset(params: &FilterParams) -> i64 {
        params.tz_offset.unwrap_or(0)
    }

    fn index_context_file(
        dir: &Path,
        conv_id: &str,
        index: &mut HashMap<String, (String, String)>,
    ) {
        let path = dir.join("context.json");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(ctx) = serde_json::from_str::<ContextFile>(&content) else {
            log::warn!("[BYOK] 解析 context.json 失败: {}", path.display());
            return;
        };
        for item in ctx.items {
            if item.kind != "metadata" || item.request_id.is_empty() {
                continue;
            }
            let Some(payload) = item.payload else {
                continue;
            };
            if payload.get("type").and_then(|v| v.as_str()) != Some("run_request") {
                continue;
            }
            let model = payload
                .get("value")
                .and_then(|v| v.get("model_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !model.is_empty() {
                index.insert(item.request_id, (conv_id.to_string(), model));
            }
        }
    }

    fn index_state_file(dir: &Path, fallback: &mut HashMap<String, String>) {
        let path = dir.join("state.json");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(state) = serde_json::from_str::<StateFile>(&content) else {
            return;
        };
        for call in [state.latest_request_prefix, state.last_provider_call]
            .into_iter()
            .flatten()
        {
            if !call.request_id.is_empty() && !call.model.is_empty() {
                fallback.entry(call.request_id).or_insert(call.model);
            }
        }
    }
}

fn record_matches_params(record: &RawRecord, params: &FilterParams) -> bool {
    if let Some(from) = params.from_epoch {
        if from > 0 && record.created_at < from {
            return false;
        }
    }
    if let Some(to) = params.to_epoch {
        if to > 0 && record.created_at >= to {
            return false;
        }
    }
    if let Some(ref provider_id) = params.provider_id {
        if !provider_id.is_empty() && record.provider_id != *provider_id {
            return false;
        }
    }
    if let Some(ref model_id) = params.model_id {
        if !model_id.is_empty() && record.model != *model_id {
            return false;
        }
    }
    true
}

/// 解析 UTC ISO8601 时间戳（可含任意位小数与尾部 Z）为 Unix 秒
fn parse_iso_to_epoch(s: &str) -> i64 {
    use chrono::{NaiveDateTime, TimeZone, Utc};
    let t = s.split('.').next().unwrap_or(s).trim_end_matches('Z');
    NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S")
        .map(|dt| Utc.from_utc_datetime(&dt).timestamp())
        .unwrap_or(0)
}

/// 检测目录是否为 Cursor-BYOK history 目录（含 usage.json）
pub fn detect_cursor_byok_history(path: &str) -> bool {
    Path::new(path).join(USAGE_FILE_NAME).is_file()
}

impl DataSource for CursorByokDbService {
    fn open(&mut self, path: &str) -> Result<(), String> {
        CursorByokDbService::open(self, path)
    }

    fn close(&mut self) {
        CursorByokDbService::close(self);
    }

    fn is_open(&self) -> bool {
        CursorByokDbService::is_open(self)
    }

    fn get_record_count(&self) -> Result<i64, String> {
        Ok(self.records_read()?.len() as i64)
    }

    fn get_latest_timestamp(&self) -> Option<i64> {
        self.latest_timestamp
    }

    fn get_providers(&self) -> Result<Vec<Provider>, String> {
        Ok(vec![Provider {
            id: PROVIDER_ID.to_string(),
            name: PROVIDER_NAME.to_string(),
        }])
    }

    fn get_models(&self) -> Result<Vec<String>, String> {
        let records = self.records_read()?;
        let mut models: HashSet<String> = records.iter().map(|r| r.model.clone()).collect();
        let mut v: Vec<String> = models.drain().collect();
        v.sort();
        Ok(v)
    }

    fn get_date_range(&self) -> Result<DateRange, String> {
        let records = self.records_read()?;
        if records.is_empty() {
            return Ok(DateRange { min: 0, max: 0 });
        }
        let min = records.iter().map(|r| r.created_at).min().unwrap_or(0);
        let max = records.iter().map(|r| r.created_at).max().unwrap_or(0);
        Ok(DateRange { min, max })
    }

    fn get_summary(&self, params: &FilterParams) -> Result<SummaryData, String> {
        Ok(aggregate_summary(&self.filter_records(params)))
    }

    fn get_model_breakdown(&self, params: &FilterParams) -> Result<Vec<ModelBreakdown>, String> {
        Ok(aggregate_model_breakdown(&self.filter_records(params)))
    }

    fn get_provider_breakdown(&self, params: &FilterParams) -> Result<Vec<ProviderBreakdown>, String> {
        let names = HashMap::from([(PROVIDER_ID.to_string(), PROVIDER_NAME.to_string())]);
        Ok(aggregate_provider_breakdown(
            &self.filter_records(params),
            &names,
        ))
    }

    fn get_combined_breakdown(&self, params: &FilterParams) -> Result<Vec<CombinedBreakdownRow>, String> {
        Ok(aggregate_combined_records(
            &self.filter_records(params),
            Self::tz_offset(params),
        ))
    }

    fn get_provider_model_tokens(&self, params: &FilterParams) -> Result<Vec<ProviderModelToken>, String> {
        Ok(aggregate_provider_model_tokens(&self.filter_records(params)))
    }

    fn get_daily_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> {
        Ok(aggregate_daily_trend(
            &self.filter_records(params),
            Self::tz_offset(params),
        ))
    }

    fn get_hourly_trend(&self, params: &FilterParams) -> Result<Vec<DailyTrendRow>, String> {
        Ok(aggregate_hourly_trend(
            &self.filter_records(params),
            Self::tz_offset(params),
        ))
    }

    fn get_session_breakdown(&self, params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> {
        Ok(aggregate_session_breakdown(&self.filter_records(params)))
    }

    fn get_session_max_context_widths(&self, ids: &[String]) -> Result<HashMap<String, i64>, String> {
        let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let records = self.records_read()?;
        let mut map: HashMap<String, i64> = HashMap::new();
        for r in records.iter() {
            if !id_set.contains(r.session_id.as_str()) {
                continue;
            }
            let width = r.input_tokens + r.cache_read;
            map.entry(r.session_id.clone())
                .and_modify(|v| {
                    if width > *v {
                        *v = width;
                    }
                })
                .or_insert(width);
        }
        Ok(map)
    }

    fn get_session_model_tokens(&self, params: &FilterParams) -> Result<Vec<SessionModelToken>, String> {
        Ok(aggregate_session_model_tokens(&self.filter_records(params)))
    }

    fn get_session_request_tokens(&self, params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> {
        Ok(self
            .filter_records(params)
            .into_iter()
            .map(|r| SessionRequestToken {
                session_id: r.session_id,
                model: r.model,
                provider_id: r.provider_id,
                created_at: r.created_at,
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
                cache_read: r.cache_read,
                cache_creation: r.cache_creation,
            })
            .collect())
    }

    fn get_session_request_tokens_for_ids(
        &self,
        params: &FilterParams,
        session_ids: &[String],
    ) -> Result<Vec<SessionRequestToken>, String> {
        let id_set: HashSet<&str> = session_ids.iter().map(|s| s.as_str()).collect();
        Ok(self
            .filter_records(params)
            .into_iter()
            .filter(|r| id_set.contains(r.session_id.as_str()))
            .map(|r| SessionRequestToken {
                session_id: r.session_id,
                model: r.model,
                provider_id: r.provider_id,
                created_at: r.created_at,
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
                cache_read: r.cache_read,
                cache_creation: r.cache_creation,
            })
            .collect())
    }

    fn get_session_model_tokens_for_ids(
        &self,
        params: &FilterParams,
        session_ids: &[String],
    ) -> Result<Vec<SessionModelToken>, String> {
        let id_set: HashSet<&str> = session_ids.iter().map(|s| s.as_str()).collect();
        let filtered: Vec<RawRecord> = self
            .filter_records(params)
            .into_iter()
            .filter(|r| id_set.contains(r.session_id.as_str()))
            .collect();
        Ok(aggregate_session_model_tokens(&filtered))
    }

    fn get_session_timestamps(&self, ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String> {
        let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let records = self.records_read()?;
        let mut map: HashMap<String, Vec<i64>> = HashMap::new();
        for r in records.iter() {
            if id_set.contains(r.session_id.as_str()) {
                map.entry(r.session_id.clone())
                    .or_default()
                    .push(r.created_at);
            }
        }
        for timestamps in map.values_mut() {
            timestamps.sort_unstable();
        }
        Ok(map)
    }

    fn get_model_context_tier_buckets(
        &self,
        params: &FilterParams,
        thresholds: &[i64],
    ) -> Result<Vec<ModelContextTierBucket>, String> {
        Ok(aggregate_model_context_tier_buckets(
            &self.filter_records(params),
            Self::tz_offset(params),
            thresholds,
            None,
        ))
    }

    fn get_minute_level_token_trend(&self) -> Result<Vec<RealtimeBucket>, String> {
        let now = utils::now_epoch_seconds();
        let since = now - REALTIME_WINDOW_SEC;
        let records = self.records_read()?;
        let mut map: HashMap<i64, RealtimeBucket> = HashMap::new();
        for r in records.iter() {
            if r.created_at < since {
                continue;
            }
            let bucket = (r.created_at / 60) * 60;
            map.entry(bucket)
                .and_modify(|b| {
                    b.requests += 1;
                    b.input_tokens += r.input_tokens;
                    b.output_tokens += r.output_tokens;
                    b.cache_read += r.cache_read;
                    b.cache_creation += r.cache_creation;
                })
                .or_insert(RealtimeBucket {
                    bucket,
                    requests: 1,
                    input_tokens: r.input_tokens,
                    output_tokens: r.output_tokens,
                    cache_read: r.cache_read,
                    cache_creation: r.cache_creation,
                });
        }
        let mut v: Vec<_> = map.into_values().collect();
        v.sort_by_key(|b| b.bucket);
        Ok(v)
    }

    fn get_recent_request_logs_raw(
        &self,
        since: Option<i64>,
    ) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64, bool)>, String> {
        let records = self.records_read()?;
        let mut rows: Vec<_> = records
            .iter()
            .filter(|r| since.map(|s| r.created_at > s).unwrap_or(true))
            .map(|r| {
                (
                    r.session_id.clone(),
                    r.model.clone(),
                    r.provider_id.clone(),
                    r.created_at,
                    r.input_tokens,
                    r.output_tokens,
                    r.cache_read,
                    r.cache_creation,
                    r.latency,
                    false,
                )
            })
            .collect();
        rows.sort_by(|a, b| b.3.cmp(&a.3));
        if since.is_none() {
            rows.truncate(SESSION_TOP_N as usize);
        }
        Ok(rows)
    }

    fn get_filtered_records(&self, params: &FilterParams) -> Result<Vec<RawRecord>, String> {
        Ok(self.filter_records(params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    fn build_fixture_dir() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let conv_dir = dir.path().join("conv-aaaa");
        std::fs::create_dir_all(&conv_dir).unwrap();
        let history = dir.path().to_str().unwrap().to_string();
        (dir, history)
    }

    fn sample_usage_json() -> &'static str {
        r#"{
  "schema_version": 2,
  "totals": { "provider_calls": 2, "turns_total": 1 },
  "recent_events": [
    {
      "event_id": "req-111::call-1",
      "kind": "provider_call",
      "at": "2026-08-07T03:23:12.6613651Z",
      "input_tokens": 100,
      "output_tokens": 20,
      "cache_read_tokens": 300,
      "cache_write_tokens": 5,
      "total_tokens": 425,
      "usage_present": true
    },
    {
      "event_id": "req-222::call-2",
      "kind": "provider_call",
      "at": "2026-08-07T03:24:00Z",
      "input_tokens": 10,
      "output_tokens": 2,
      "cache_read_tokens": 0,
      "cache_write_tokens": 0,
      "total_tokens": 12,
      "usage_present": false
    },
    {
      "event_id": "turn::conv-aaaa::3",
      "kind": "turn_finalized",
      "status": "completed",
      "at": "2026-08-07T03:25:00Z",
      "input_tokens": 110,
      "output_tokens": 22,
      "cache_read_tokens": 300,
      "cache_write_tokens": 5,
      "total_tokens": 437,
      "usage_present": true
    }
  ]
}"#
    }

    #[test]
    fn test_parse_and_map() {
        let (dir, history) = build_fixture_dir();
        let conv_dir = dir.path().join("conv-aaaa");
        write_file(
            &conv_dir,
            "context.json",
            r#"{
  "conversation_id": "conv-aaaa",
  "items": [
    {
      "seq": 1,
      "request_id": "req-111",
      "kind": "request_context",
      "payload": { "env": { "workspacePaths": ["/proj/a"] } }
    },
    {
      "seq": 4,
      "request_id": "req-111",
      "kind": "metadata",
      "payload": { "type": "run_request", "value": { "model_name": "glm-5.2" } }
    }
  ]
}"#,
        );
        write_file(dir.path(), "usage.json", sample_usage_json());

        let mut source = CursorByokDbService::new();
        source.open(&history).unwrap();
        assert_eq!(source.get_record_count().unwrap(), 1);
        let records = source.records_read().unwrap();
        assert_eq!(records[0].model, "glm-5.2");
        assert_eq!(records[0].provider_id, "cursor-byok");
        assert_eq!(records[0].session_id, "byok-conv-aaaa");
        assert_eq!(records[0].created_at, parse_iso_to_epoch("2026-08-07T03:23:12.6613651Z"));
        assert_eq!(records[0].input_tokens, 100);
        assert_eq!(records[0].output_tokens, 20);
        assert_eq!(records[0].cache_read, 300);
        assert_eq!(records[0].cache_creation, 5);

        let models = source.get_models().unwrap();
        assert_eq!(models, vec!["glm-5.2"]);
        let providers = source.get_providers().unwrap();
        assert_eq!(providers[0].id, "cursor-byok");

        let range = source.get_date_range().unwrap();
        assert_eq!(range.min, parse_iso_to_epoch("2026-08-07T03:23:12.6613651Z"));
        assert_eq!(range.max, parse_iso_to_epoch("2026-08-07T03:23:12.6613651Z"));
    }

    #[test]
    fn test_state_fallback_and_summary() {
        let (dir, history) = build_fixture_dir();
        let conv_dir = dir.path().join("conv-aaaa");
        // context.json 无 run_request（模型映射缺失），state.json 兜底
        write_file(
            &conv_dir,
            "context.json",
            r#"{"conversation_id":"conv-aaaa","items":[]}"#,
        );
        write_file(
            &conv_dir,
            "state.json",
            r#"{
  "conversation_id": "conv-aaaa",
  "latest_request_prefix": { "request_id": "req-111", "model": "deepseek-v4-flash", "provider": "anthropic" }
}"#,
        );
        write_file(dir.path(), "usage.json", sample_usage_json());

        let mut source = CursorByokDbService::new();
        source.open(&history).unwrap();
        let records = source.records_read().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "deepseek-v4-flash");

        // 聚合：仅 1 条有效记录
        let summary = source.get_summary(&FilterParams { from_epoch: None, to_epoch: None, tz_offset: None, provider_id: None, model_id: None }).unwrap();
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.total_input, 100);
        assert_eq!(summary.total_cache_read, 300);
        assert_eq!(summary.total_cache_creation, 5);
    }

    #[test]
    fn test_unknown_model_without_sources() {
        let (dir, history) = build_fixture_dir();
        let conv_dir = dir.path().join("conv-aaaa");
        write_file(&conv_dir, "context.json", r#"{"items":[]}"#);
        write_file(dir.path(), "usage.json", sample_usage_json());

        let mut source = CursorByokDbService::new();
        source.open(&history).unwrap();
        let records = source.records_read().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "unknown");
    }

    #[test]
    fn test_detect_history_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!detect_cursor_byok_history(dir.path().to_str().unwrap()));
        write_file(dir.path(), "usage.json", "{}");
        assert!(detect_cursor_byok_history(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_reopen_skips_when_unchanged() {
        let (dir, history) = build_fixture_dir();
        let conv_dir = dir.path().join("conv-aaaa");
        write_file(
            &conv_dir,
            "context.json",
            r#"{"conversation_id":"conv-aaaa","items":[{"request_id":"req-111","kind":"metadata","payload":{"type":"run_request","value":{"model_name":"glm-5.2"}}}]}"#,
        );
        write_file(dir.path(), "usage.json", sample_usage_json());

        let mut source = CursorByokDbService::new();
        source.open(&history).unwrap();
        // 二次 open 且 usage.json 未变化：跳过重解析，记录数不变
        source.open(&history).unwrap();
        assert_eq!(source.get_record_count().unwrap(), 1);
    }
}