//! Proma 本地会话数据源（只读）
//!
//! 读取 `~/.proma/agent-sessions/*.jsonl` 的 Agent 会话消息，提取 assistant
//! 消息的 token 用量。目录型数据源，模式仿 Cursor（全量加载内存聚合）。
//!
//! 关键差异（相对 SQLite 类数据源）：
//! - 每行 JSON 兼容两种格式：
//!   1. SDK 格式（Phase 4 起）：顶层 `type: "assistant"`，usage 在
//!      `message.usage`（snake_case），时间戳 `_createdAt` 毫秒
//!   2. 旧 AgentMessage 格式（历史数据）：顶层 `role: "assistant"`，
//!      usage 在顶层 `usage`（camelCase），时间戳 `createdAt` 毫秒
//! - 同一消息 id 有多个流式分片行（thinking/text/tool_use），usage 为
//!   快照重复，需按 id 保留 usage 总和最大的一条
//! - 时间戳为毫秒，需 /1000 转秒
//! - provider 统一为 "proma"；session_id 置空，不参与会话归类（与 ZCode 一致）
//! - 不读取 sdk-config/sessions（SDK 运行时目录，与 agent-sessions 重复）

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::RwLock;

use serde_json::Value;

use crate::models::*;
use crate::services::data_source::DataSource;
use crate::services::pipeline::{
    aggregate_combined_records, aggregate_daily_trend, aggregate_hourly_trend,
    aggregate_model_breakdown, aggregate_model_context_tier_buckets, aggregate_provider_breakdown,
    aggregate_provider_model_tokens, aggregate_summary,
};
use crate::utils::{self, REALTIME_WINDOW_SEC, SESSION_TOP_N};

const PROVIDER_ID: &str = "proma";
const PROVIDER_NAME: &str = "Proma";

pub struct PromaDirService {
    dir_path: String,
    records: RwLock<Vec<RawRecord>>,
    latest_timestamp: Option<i64>,
}

impl PromaDirService {
    pub fn new() -> Self {
        Self {
            dir_path: String::new(),
            records: RwLock::new(Vec::new()),
            latest_timestamp: None,
        }
    }

    pub fn open(&mut self, dir_path: &str) -> Result<(), String> {
        self.close();
        if !detect_proma_dir(dir_path) {
            return Err(format!("Proma 数据目录无效（未找到 agent-sessions）: {}", dir_path));
        }
        let parsed = parse_proma_sessions_dir(Path::new(dir_path))?;
        log::info!("[PROMA] 解析完成: 会话目录={} 记录数={}", dir_path, parsed.len());
        self.latest_timestamp = parsed.iter().map(|r| r.created_at).max();
        *self.records.write().map_err(|e| format!("数据锁失败: {}", e))? = parsed;
        self.dir_path = dir_path.to_string();
        Ok(())
    }

    pub fn close(&mut self) {
        self.dir_path.clear();
        if let Ok(mut guard) = self.records.write() {
            guard.clear();
        }
        self.latest_timestamp = None;
    }

    pub fn is_open(&self) -> bool {
        !self.dir_path.is_empty()
    }

    fn records_read(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<RawRecord>>, String> {
        self.records.read().map_err(|e| format!("数据锁失败: {}", e))
    }

    fn filter_records(&self, params: &FilterParams) -> Vec<RawRecord> {
        let records = match self.records_read() {
            Ok(guard) => guard,
            Err(e) => {
                log::warn!("[PROMA] 读取记录失败: {}", e);
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

/// 判定目录是否为 Proma 数据目录：存在 `agent-sessions/`（含 jsonl）
/// 或 `agent-sessions.json` 索引文件。
pub fn detect_proma_dir(path: &str) -> bool {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return false;
    }
    if dir.join("agent-sessions.json").is_file() {
        return true;
    }
    let sessions_dir = dir.join("agent-sessions");
    if !sessions_dir.is_dir() {
        return false;
    }
    std::fs::read_dir(&sessions_dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    == Some("jsonl")
            })
        })
        .unwrap_or(false)
}

/// 解析 `~/.proma/agent-sessions/*.jsonl` 全部会话文件，返回 RawRecord 列表。
/// 同一消息 id 的多行流式分片按 usage 总和最大的一条保留。
pub fn parse_proma_sessions_dir(dir: &Path) -> Result<Vec<RawRecord>, String> {
    let sessions_dir = dir.join("agent-sessions");
    if !sessions_dir.is_dir() {
        return Err(format!("agent-sessions 目录不存在: {}", sessions_dir.display()));
    }

    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&sessions_dir)
        .map_err(|e| format!("读取 agent-sessions 目录失败: {}", e))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .collect();
    files.sort();

    // key → 记录；key 为消息 id（SDK 格式 message.id / 旧格式顶层 id），
    // 缺失时回退 uuid，再缺失按文件+时间戳兜底，保证不被误去重
    let mut by_id: HashMap<String, RawRecord> = HashMap::new();
    for file in &files {
        let Ok(content) = std::fs::read_to_string(file) else {
            log::warn!("[PROMA] 读取会话文件失败: {}", file.display());
            continue;
        };
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            parse_proma_line(line, by_id.len(), &mut by_id);
        }
    }

    let mut records: Vec<RawRecord> = by_id.into_values().collect();
    records.sort_by_key(|r| r.created_at);
    Ok(records)
}

/// 解析单行 JSON，兼容 SDK 格式（顶层 type）与旧 AgentMessage 格式（顶层 role）。
fn parse_proma_line(line: &str, fallback_seq: usize, by_id: &mut HashMap<String, RawRecord>) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return;
    };
    match v.get("type").and_then(Value::as_str) {
        Some("assistant") => parse_sdk_message(&v, fallback_seq, by_id),
        Some(_) => {} // user / session / model_change 等非消息行
        None => {
            if v.get("role").and_then(Value::as_str) == Some("assistant") {
                parse_agent_message(&v, fallback_seq, by_id);
            }
        }
    }
}

/// 按 usage 总和保留较大者写入去重表
fn upsert_by_total(by_id: &mut HashMap<String, RawRecord>, key: String, record: RawRecord) {
    let total = record.input_tokens
        + record.output_tokens
        + record.cache_read
        + record.cache_creation;
    by_id
        .entry(key)
        .and_modify(|existing| {
            let existing_total = existing.input_tokens
                + existing.output_tokens
                + existing.cache_read
                + existing.cache_creation;
            if total > existing_total {
                *existing = record.clone();
            }
        })
        .or_insert(record);
}

/// SDK 格式：`{type:"assistant", message:{id,model,usage:{...}}, _createdAt, _channelModelId, uuid}`
fn parse_sdk_message(v: &Value, fallback_seq: usize, by_id: &mut HashMap<String, RawRecord>) {
    let Some(msg) = v.get("message") else { return };
    let Some(usage) = msg.get("usage") else { return };
    let input = usage.get("input_tokens").and_then(Value::as_i64).unwrap_or(0);
    let output = usage.get("output_tokens").and_then(Value::as_i64).unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if input <= 0 && output <= 0 && cache_read <= 0 && cache_creation <= 0 {
        return;
    }
    // 模型口径：实际响应模型 message.model，缺失回退渠道配置 _channelModelId
    let model = msg
        .get("model")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            v.get("_channelModelId")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_default();
    let created_ms = v.get("_createdAt").and_then(Value::as_i64).unwrap_or(0);
    if created_ms <= 0 {
        return;
    }
    let key = msg
        .get("id")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| v.get("uuid").and_then(Value::as_str).map(String::from))
        .unwrap_or_else(|| format!("sdk-{}-{}", fallback_seq, created_ms));
    upsert_by_total(
        by_id,
        key,
        RawRecord {
            session_id: String::new(), // Proma 不参与会话归类
            model,
            provider_id: PROVIDER_ID.to_string(),
            created_at: created_ms / 1000,
            input_tokens: input,
            output_tokens: output,
            cache_read,
            cache_creation,
            latency: 0,
            is_codex: false,
        },
    );
}

/// 旧 AgentMessage 格式：`{id, role:"assistant", content, createdAt, model, usage:{...camelCase}, durationMs}`
fn parse_agent_message(v: &Value, fallback_seq: usize, by_id: &mut HashMap<String, RawRecord>) {
    let Some(usage) = v.get("usage") else { return };
    let input = usage.get("inputTokens").and_then(Value::as_i64).unwrap_or(0);
    let output = usage.get("outputTokens").and_then(Value::as_i64).unwrap_or(0);
    let cache_read = usage.get("cacheReadTokens").and_then(Value::as_i64).unwrap_or(0);
    let cache_creation = usage
        .get("cacheCreationTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if input <= 0 && output <= 0 && cache_read <= 0 && cache_creation <= 0 {
        return;
    }
    let model = v.get("model").and_then(Value::as_str).unwrap_or_default().to_string();
    let created_ms = v.get("createdAt").and_then(Value::as_i64).unwrap_or(0);
    if created_ms <= 0 {
        return;
    }
    let key = v
        .get("id")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| format!("msg-{}-{}", fallback_seq, created_ms));
    upsert_by_total(
        by_id,
        key,
        RawRecord {
            session_id: String::new(),
            model,
            provider_id: PROVIDER_ID.to_string(),
            created_at: created_ms / 1000,
            input_tokens: input,
            output_tokens: output,
            cache_read,
            cache_creation,
            latency: v.get("durationMs").and_then(Value::as_i64).unwrap_or(0),
            is_codex: false,
        },
    );
}

impl DataSource for PromaDirService {
    fn open(&mut self, path: &str) -> Result<(), String> {
        PromaDirService::open(self, path)
    }

    fn close(&mut self) {
        PromaDirService::close(self);
    }

    fn is_open(&self) -> bool {
        PromaDirService::is_open(self)
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
        Ok(aggregate_provider_breakdown(&self.filter_records(params), &names))
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

    /// 与 ZCode 一致：会话 tab 不展示 Proma 会话（session_id 置空，不参与会话归类）
    fn get_session_breakdown(&self, _params: &FilterParams) -> Result<Vec<SessionBreakdown>, String> {
        Ok(Vec::new())
    }

    fn get_session_max_context_widths(&self, _ids: &[String]) -> Result<HashMap<String, i64>, String> {
        Ok(HashMap::new())
    }

    fn get_session_model_tokens(&self, _params: &FilterParams) -> Result<Vec<SessionModelToken>, String> {
        Ok(Vec::new())
    }

    fn get_session_request_tokens(&self, _params: &FilterParams) -> Result<Vec<SessionRequestToken>, String> {
        Ok(Vec::new())
    }

    fn get_session_request_tokens_for_ids(
        &self,
        _params: &FilterParams,
        _session_ids: &[String],
    ) -> Result<Vec<SessionRequestToken>, String> {
        Ok(Vec::new())
    }

    fn get_session_model_tokens_for_ids(
        &self,
        _params: &FilterParams,
        _session_ids: &[String],
    ) -> Result<Vec<SessionModelToken>, String> {
        Ok(Vec::new())
    }

    fn get_session_timestamps(&self, _ids: &[String]) -> Result<HashMap<String, Vec<i64>>, String> {
        Ok(HashMap::new())
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
    use std::io::Write;

    /// 构造临时 Proma 数据目录：`agent-sessions/` 下写入若干 jsonl 文件
    fn write_temp_sessions(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("agent-sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        for (name, content) in files {
            let path = sessions_dir.join(name);
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(content.as_bytes()).unwrap();
        }
        dir
    }

    fn sdk_assistant(msg_id: &str, model: &str, ts_ms: i64, input: i64, output: i64) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"id":"{}","role":"assistant","content":[],"model":"{}","usage":{{"input_tokens":{},"output_tokens":{},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}},"parent_tool_use_id":null,"uuid":"u-{}","_channelModelId":"claude-opus-4-8","_createdAt":{}}}"#,
            msg_id, model, input, output, msg_id, ts_ms
        )
    }

    #[test]
    fn test_detect_proma_dir() {
        let empty = tempfile::tempdir().unwrap();
        assert!(!detect_proma_dir(empty.path().to_str().unwrap()));
        assert!(!detect_proma_dir("Z:/nonexistent/path"));

        // 只有索引文件也识别（索引指向的会话文件可能在外部/已清理）
        let index_only = tempfile::tempdir().unwrap();
        std::fs::write(index_only.path().join("agent-sessions.json"), "{}").unwrap();
        assert!(detect_proma_dir(index_only.path().to_str().unwrap()));

        // agent-sessions 目录 + jsonl
        let dir = write_temp_sessions(&[("s1.jsonl", "{}\n")]);
        assert!(detect_proma_dir(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_parse_sdk_format() {
        let dir = write_temp_sessions(&[(
            "s1.jsonl",
            &format!(
                "{}\n{}\n",
                sdk_assistant("m1", "MiniMax-M3", 1786592741238, 100, 50),
                r#"{"type":"user","message":{"content":[]},"uuid":"u-user","_createdAt":1786592740000}"#
            ),
        )]);
        let records = parse_proma_sessions_dir(dir.path()).unwrap();
        assert_eq!(records.len(), 1, "user 行不应产生记录");
        let r = &records[0];
        assert_eq!(r.model, "MiniMax-M3");
        assert_eq!(r.provider_id, "proma");
        assert_eq!(r.created_at, 1786592741238 / 1000);
        assert_eq!(r.input_tokens, 100);
        assert_eq!(r.output_tokens, 50);
        assert_eq!(r.session_id, "");
    }

    #[test]
    fn test_parse_agent_message_legacy_format() {
        let dir = write_temp_sessions(&[(
            "s2.jsonl",
            r#"{"id":"old-1","role":"assistant","content":"hi","createdAt":1781000000000,"model":"gpt-4o","durationMs":321,"usage":{"inputTokens":10,"outputTokens":20,"cacheReadTokens":3,"cacheCreationTokens":4}}
{"id":"old-2","role":"user","content":"q","createdAt":1781000000000}"#,
        )]);
        let records = parse_proma_sessions_dir(dir.path()).unwrap();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.model, "gpt-4o");
        assert_eq!(r.input_tokens, 10);
        assert_eq!(r.cache_read, 3);
        assert_eq!(r.cache_creation, 4);
        assert_eq!(r.latency, 321);
        assert_eq!(r.created_at, 1781000000000 / 1000);
    }

    #[test]
    fn test_parse_fragment_dedup_keeps_max_total() {
        // 同一消息 id 的流式分片，usage 为快照，取总和最大的一条
        let dir = write_temp_sessions(&[(
            "s3.jsonl",
            &format!(
                "{}\n{}\n",
                sdk_assistant("m9", "m1-model", 1786592741000, 10, 0),
                sdk_assistant("m9", "m1-model", 1786592742000, 10, 40)
            ),
        )]);
        let records = parse_proma_sessions_dir(dir.path()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].output_tokens, 40);
    }

    #[test]
    fn test_parse_skips_zero_usage_and_error() {
        let dir = write_temp_sessions(&[(
            "s4.jsonl",
            r#"{"type":"assistant","message":{"id":"e1","role":"assistant","content":[],"model":"claude-opus-4-8","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}},"uuid":"u-e1","_channelModelId":"claude-opus-4-8","_createdAt":1786592741238}
{"type":"assistant","message":{"id":"e2","role":"assistant","content":[],"model":"claude-opus-4-8","stop_reason":"error"},"uuid":"u-e2","_channelModelId":"claude-opus-4-8","_createdAt":1786592742238}
not-json-garbage
"#,
        )]);
        let records = parse_proma_sessions_dir(dir.path()).unwrap();
        assert!(records.is_empty(), "全 0 usage / 无 usage / 坏行都应跳过");
    }

    #[test]
    fn test_parse_model_fallback_to_channel_model() {
        let dir = write_temp_sessions(&[(
            "s5.jsonl",
            r#"{"type":"assistant","message":{"id":"m-fb","role":"assistant","content":[],"usage":{"input_tokens":5,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}},"uuid":"u-fb","_channelModelId":"claude-opus-4-8","_createdAt":1786592741238}"#,
        )]);
        let records = parse_proma_sessions_dir(dir.path()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "claude-opus-4-8");
    }

    #[test]
    fn test_open_and_aggregate() {
        let dir = write_temp_sessions(&[(
            "s6.jsonl",
            &format!(
                "{}\n{}\n",
                sdk_assistant("a1", "model-x", 1786592740000, 100, 50),
                sdk_assistant("a2", "model-x", 1786592741000, 200, 10)
            ),
        )]);
        let mut source = PromaDirService::new();
        source.open(dir.path().to_str().unwrap()).unwrap();
        assert!(source.is_open());
        assert_eq!(source.get_record_count().unwrap(), 2);
        assert_eq!(source.get_latest_timestamp(), Some(1786592741000 / 1000));
        let models = source.get_models().unwrap();
        assert_eq!(models, vec!["model-x"]);
        let providers = source.get_providers().unwrap();
        assert_eq!(providers[0].id, "proma");
        assert_eq!(providers[0].name, "Proma");
        let summary = source
            .get_summary(&FilterParams {
                from_epoch: None,
                to_epoch: None,
                tz_offset: Some(8),
                provider_id: None,
                model_id: None,
            })
            .unwrap();
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.total_input, 300);
        assert_eq!(summary.total_output, 60);
        // 会话 tab 不参与
        assert!(source
            .get_session_breakdown(&FilterParams {
                from_epoch: None,
                to_epoch: None,
                tz_offset: Some(8),
                provider_id: None,
                model_id: None,
            })
            .unwrap()
            .is_empty());
        source.close();
        assert!(!source.is_open());
    }
}
