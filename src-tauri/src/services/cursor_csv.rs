use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::RwLock;

use crate::models::*;
use crate::services::cursor_attribution::{
    delete_override, explain_filter_reason, gc_overrides, load_overrides, resolve_effective,
    row_key, should_apply_attribution_for_ts, upsert_override, AttributionTokenStats,
    CursorCsvPreviewPage, CursorCsvPreviewRow, EffectiveAttribution, LocalHookEvent,
    OverrideAction, OverrideEntry, TokenQuad, ATTRIBUTION_SLACK_SECS,
};
use crate::services::cursor_local_hook;
use crate::services::data_source::DataSource;
use crate::services::pipeline::{
    aggregate_combined_records, aggregate_daily_trend, aggregate_hourly_trend,
    aggregate_model_breakdown, aggregate_model_context_tier_buckets, aggregate_provider_breakdown,
    aggregate_provider_model_tokens, aggregate_session_breakdown, aggregate_session_model_tokens,
    aggregate_summary,
};
use crate::utils::{self, SESSION_TOP_N, REALTIME_WINDOW_SEC};

const PROVIDER_ID: &str = "cursor";

pub struct CursorCsvService {
    cache_dir: String,
    /// 账号 userId（来自 account.json / 目录名）
    user_id: String,
    records: RwLock<Vec<RawRecord>>,
    latest_timestamp: Option<i64>,
    attribution_enabled: bool,
    local_events: Vec<LocalHookEvent>,
    /// 本机归因过滤起始时刻（Unix 秒），reload 时从配置读取
    attribution_filter_start: i64,
    /// row_key → 手动改判
    overrides: HashMap<String, OverrideEntry>,
}

impl CursorCsvService {
    pub fn new() -> Self {
        Self {
            cache_dir: String::new(),
            user_id: String::new(),
            records: RwLock::new(Vec::new()),
            latest_timestamp: None,
            attribution_enabled: false,
            local_events: Vec::new(),
            attribution_filter_start: cursor_local_hook::get_attribution_filter_start(),
            overrides: HashMap::new(),
        }
    }

    #[allow(dead_code)]
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    #[allow(dead_code)]
    pub fn cache_dir(&self) -> &str {
        &self.cache_dir
    }

    fn reload_attribution(&mut self) {
        self.attribution_enabled = cursor_local_hook::is_attribution_enabled();
        self.attribution_filter_start = cursor_local_hook::get_attribution_filter_start();
        if self.attribution_enabled {
            self.local_events = cursor_local_hook::load_local_events().unwrap_or_else(|e| {
                log::warn!("[CURSOR] 读取本机 Hook 日志失败: {}", e);
                Vec::new()
            });
        } else {
            self.local_events.clear();
        }
        log::info!(
            "[CURSOR] 本机归因 enabled={} filter_start={} local_events={}",
            self.attribution_enabled,
            self.attribution_filter_start,
            self.local_events.len()
        );
    }

    fn reload_overrides(&mut self) {
        if self.cache_dir.is_empty() {
            self.overrides.clear();
            return;
        }
        let dir = Path::new(&self.cache_dir);
        let mut map = load_overrides(dir).unwrap_or_else(|e| {
            log::warn!("[CURSOR] 读取改判失败: {}", e);
            HashMap::new()
        });
        let live_keys: HashSet<String> = match self.records_read() {
            Ok(records) => records
                .iter()
                .map(|r| {
                    row_key(
                        r.created_at,
                        &r.model,
                        r.input_tokens,
                        r.output_tokens,
                        r.cache_read,
                        r.cache_creation,
                    )
                })
                .collect(),
            Err(_) => HashSet::new(),
        };
        let _ = gc_overrides(dir, &mut map, &live_keys);
        self.overrides = map;
    }

    pub fn refresh_local_events(&mut self) {
        self.reload_attribution();
    }

    pub fn open(&mut self, dir_path: &str) -> Result<(), String> {
        self.close();
        let csv_path = Path::new(dir_path).join("usage.csv");
        if !csv_path.is_file() {
            return Err(format!("Cursor 缓存文件不存在: {}", csv_path.display()));
        }
        let user_id = utils::resolve_account_user_id(Path::new(dir_path));
        let parsed = parse_cursor_csv_file(&csv_path, &user_id)?;
        self.latest_timestamp = parsed.iter().map(|r| r.created_at).max();
        *self.records.write().map_err(|e| format!("数据锁失败: {}", e))? = parsed;
        self.cache_dir = dir_path.to_string();
        self.user_id = user_id;
        self.reload_attribution();
        self.reload_overrides();
        Ok(())
    }

    pub fn close(&mut self) {
        self.cache_dir.clear();
        self.user_id.clear();
        if let Ok(mut guard) = self.records.write() {
            guard.clear();
        }
        self.latest_timestamp = None;
        self.local_events.clear();
        self.overrides.clear();
        self.attribution_enabled = false;
    }

    pub fn is_open(&self) -> bool {
        !self.cache_dir.is_empty()
    }

    fn records_read(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<RawRecord>>, String> {
        self.records.read().map_err(|e| format!("数据锁失败: {}", e))
    }

    fn algo_reason_for(&self, r: &RawRecord) -> Option<crate::services::cursor_attribution::FilterReason> {
        let apply_attr = self.attribution_enabled && !self.local_events.is_empty();
        if apply_attr && should_apply_attribution_for_ts(r.created_at, self.attribution_filter_start) {
            explain_filter_reason(
                r.created_at,
                &r.model,
                &self.local_events,
                ATTRIBUTION_SLACK_SECS,
            )
        } else {
            None
        }
    }

    fn effective_for(&self, r: &RawRecord) -> (String, EffectiveAttribution) {
        let key = row_key(
            r.created_at,
            &r.model,
            r.input_tokens,
            r.output_tokens,
            r.cache_read,
            r.cache_creation,
        );
        let ov = self.overrides.get(&key).map(|e| e.action);
        let eff = resolve_effective(self.algo_reason_for(r), ov);
        (key, eff)
    }

    /// CSV 全量 token 与因本机归因被滤掉的 token（含手动过滤，排除申诉取回）。
    pub fn attribution_token_stats(&self) -> AttributionTokenStats {
        let records = match self.records_read() {
            Ok(guard) => guard,
            Err(_) => return AttributionTokenStats::default(),
        };
        let mut csv_total = TokenQuad::default();
        let mut filtered_out = TokenQuad::default();
        for r in records.iter() {
            csv_total.add_record_tokens(
                r.input_tokens,
                r.output_tokens,
                r.cache_read,
                r.cache_creation,
            );
            let (_, eff) = self.effective_for(r);
            if eff.filtered {
                filtered_out.add_record_tokens(
                    r.input_tokens,
                    r.output_tokens,
                    r.cache_read,
                    r.cache_creation,
                );
            }
        }
        AttributionTokenStats {
            csv_total,
            filtered_out,
        }
    }

    /// 分页预览 CSV。按时间降序；`filtered_only` 时仅返回最终被滤掉的行；
    /// `model_filter` 非空时按模型精确匹配。
    pub fn preview_csv(
        &self,
        page: usize,
        page_size: usize,
        filtered_only: bool,
        model_filter: Option<&str>,
    ) -> CursorCsvPreviewPage {
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let empty = CursorCsvPreviewPage {
            items: Vec::new(),
            total: 0,
            page,
            page_size,
            available_models: Vec::new(),
        };
        let records = match self.records_read() {
            Ok(guard) => guard,
            Err(_) => return empty,
        };

        // 收集索引：按 created_at 降序
        let mut indices: Vec<usize> = (0..records.len()).collect();
        indices.sort_by(|&a, &b| {
            records[b]
                .created_at
                .cmp(&records[a].created_at)
                .then_with(|| a.cmp(&b))
        });

        if filtered_only {
            indices.retain(|&i| {
                let (_, eff) = self.effective_for(&records[i]);
                eff.filtered
            });
        }

        // 模型下拉：在 model 筛选前收集 distinct
        let mut available_models: Vec<String> = indices
            .iter()
            .map(|&i| records[i].model.clone())
            .collect();
        available_models.sort();
        available_models.dedup();

        let model_filter = model_filter
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(model) = model_filter {
            indices.retain(|&i| records[i].model == model);
        }

        let total = indices.len();
        let start = (page - 1).saturating_mul(page_size);
        if start >= total {
            return CursorCsvPreviewPage {
                items: Vec::new(),
                total,
                page,
                page_size,
                available_models,
            };
        }
        let end = (start + page_size).min(total);

        let items: Vec<CursorCsvPreviewRow> = indices[start..end]
            .iter()
            .map(|&i| {
                let r = &records[i];
                let (key, eff) = self.effective_for(r);
                CursorCsvPreviewRow {
                    created_at: r.created_at,
                    model: r.model.clone(),
                    input: r.input_tokens,
                    output: r.output_tokens,
                    cache_read: r.cache_read,
                    cache_creation: r.cache_creation,
                    filtered: eff.filtered,
                    reason: eff.reason,
                    row_key: key,
                    override_action: eff.override_action,
                    user_id: if self.user_id.is_empty() {
                        None
                    } else {
                        Some(self.user_id.clone())
                    },
                    cache_path: if self.cache_dir.is_empty() {
                        None
                    } else {
                        Some(self.cache_dir.clone())
                    },
                }
            })
            .collect();

        CursorCsvPreviewPage {
            items,
            total,
            page,
            page_size,
            available_models,
        }
    }

    pub fn set_attribution_override(
        &mut self,
        key: &str,
        action: OverrideAction,
        created_at: i64,
        model: &str,
    ) -> Result<(), String> {
        if self.cache_dir.is_empty() {
            return Err("Cursor 数据源未加载".to_string());
        }
        upsert_override(
            Path::new(&self.cache_dir),
            &mut self.overrides,
            key,
            action,
            created_at,
            model,
        )
    }

    pub fn clear_attribution_override(&mut self, key: &str) -> Result<(), String> {
        if self.cache_dir.is_empty() {
            return Err("Cursor 数据源未加载".to_string());
        }
        delete_override(Path::new(&self.cache_dir), &mut self.overrides, key)
    }

    fn filter_records(&self, params: &FilterParams) -> Vec<RawRecord> {
        let records = match self.records_read() {
            Ok(guard) => guard,
            Err(e) => {
                log::warn!("[CURSOR] 读取记录失败: {}", e);
                return Vec::new();
            }
        };
        records
            .iter()
            .filter(|r| record_matches_params(r, params))
            .filter(|r| {
                let (_, eff) = self.effective_for(r);
                !eff.filtered
            })
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

/// 解析 Cursor usage.csv 为 RawRecord 列表；`user_id` 写入 session_id 以避免跨账号去重冲突
pub fn parse_cursor_csv_file(path: &Path, user_id: &str) -> Result<Vec<RawRecord>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 Cursor CSV 失败: {}", e))?;

    let mut lines = content.lines();
    let header = lines.next().ok_or_else(|| "Cursor CSV 为空".to_string())?;
    if !header.contains("Date") || !header.contains("Model") {
        return Err("无效的 Cursor CSV 表头".to_string());
    }

    let header_fields: Vec<&str> = parse_csv_line(header);
    let has_kind_column = header_fields.iter().any(|f| f.trim() == "Kind");
    let column_count = header_fields.len();
    let (
        model_idx,
        input_cache_write_idx,
        input_no_cache_idx,
        cache_read_idx,
        output_idx,
        _cost_idx,
    ) = if has_kind_column && column_count >= 11 {
        (4, 6, 7, 8, 9, 11)
    } else if has_kind_column {
        (2, 4, 5, 6, 7, 9)
    } else {
        (1, 2, 3, 4, 5, 7)
    };

    let uid = if user_id.trim().is_empty() {
        "_unknown"
    } else {
        user_id.trim()
    };

    let mut records = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = parse_csv_line(line);
        let min_fields = _cost_idx + 1;
        if fields.len() < min_fields {
            continue;
        }

        let date_str = fields[0].trim().trim_matches('"');
        let model = fields[model_idx].trim().trim_matches('"');
        if model.is_empty() {
            continue;
        }

        let input_with_cache_write: i64 = fields[input_cache_write_idx]
            .trim()
            .trim_matches('"')
            .parse()
            .unwrap_or(0);
        let input_without_cache_write: i64 = fields[input_no_cache_idx]
            .trim()
            .trim_matches('"')
            .parse()
            .unwrap_or(0);
        let cache_read: i64 = fields[cache_read_idx]
            .trim()
            .trim_matches('"')
            .parse()
            .unwrap_or(0);
        let output_tokens: i64 = fields[output_idx]
            .trim()
            .trim_matches('"')
            .parse()
            .unwrap_or(0);

        let created_at = parse_date_to_epoch_secs(date_str);
        if created_at == 0 {
            continue;
        }

        let cache_creation = (input_with_cache_write - input_without_cache_write).max(0);
        let day_key = date_str.get(..10).unwrap_or(date_str);
        records.push(RawRecord {
            session_id: format!("cursor-{}-{}", uid, day_key),
            model: model.to_string(),
            provider_id: PROVIDER_ID.to_string(),
            created_at,
            input_tokens: input_without_cache_write.max(0),
            output_tokens: output_tokens.max(0),
            cache_read: cache_read.max(0),
            cache_creation,
            latency: 0,
            is_codex: false,
        });
    }
    Ok(records)
}

fn parse_csv_line(line: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    for (i, byte) in line.as_bytes().iter().enumerate() {
        match byte {
            b'"' => in_quotes = !in_quotes,
            b',' if !in_quotes => {
                fields.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start <= line.len() {
        fields.push(&line[start..]);
    }
    fields
}

pub(crate) fn parse_date_to_epoch_secs(date_str: &str) -> i64 {
    use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};

    if let Ok(dt) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S%.3fZ") {
        return Utc.from_utc_datetime(&dt).timestamp();
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%SZ") {
        return Utc.from_utc_datetime(&dt).timestamp();
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S%.3f") {
        return Utc.from_utc_datetime(&dt).timestamp();
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S") {
        return Utc.from_utc_datetime(&dt).timestamp();
    }
    if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        let dt = date.and_hms_opt(12, 0, 0).unwrap();
        return Utc.from_utc_datetime(&dt).timestamp();
    }
    0
}

pub fn detect_cursor_cache(path: &str) -> bool {
    let csv_path = Path::new(path).join("usage.csv");
    if !csv_path.is_file() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(&csv_path) else {
        return false;
    };
    let Some(header) = content.lines().next() else {
        return false;
    };
    header.contains("Date") && header.contains("Model")
}

impl DataSource for CursorCsvService {
    fn open(&mut self, path: &str) -> Result<(), String> {
        CursorCsvService::open(self, path)
    }

    fn close(&mut self) {
        CursorCsvService::close(self);
    }

    fn is_open(&self) -> bool {
        CursorCsvService::is_open(self)
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
            name: "Cursor".to_string(),
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
        let names = HashMap::from([(PROVIDER_ID.to_string(), "Cursor".to_string())]);
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

    fn get_cursor_attribution_stats(&self) -> Option<AttributionTokenStats> {
        Some(self.attribution_token_stats())
    }

    fn refresh_cursor_local_events(&mut self) {
        self.refresh_local_events();
    }

    fn get_cursor_csv_preview(
        &self,
        page: usize,
        page_size: usize,
        filtered_only: bool,
        model_filter: Option<&str>,
    ) -> Option<CursorCsvPreviewPage> {
        Some(self.preview_csv(page, page_size, filtered_only, model_filter))
    }

    fn set_cursor_attribution_override(
        &mut self,
        row_key: &str,
        action: OverrideAction,
        created_at: i64,
        model: &str,
    ) -> Result<(), String> {
        self.set_attribution_override(row_key, action, created_at, model)
    }

    fn clear_cursor_attribution_override(&mut self, row_key: &str) -> Result<(), String> {
        self.clear_attribution_override(row_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_csv(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.csv");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        (dir, path)
    }

    #[test]
    fn test_parse_cursor_csv_v1() {
        let csv = "Date,Model,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost,Cost to you
2025-02-01,gpt-4o,10,5,0,15,30,$0.10,$0.10";
        let (_dir, path) = write_temp_csv(csv);
        let records = parse_cursor_csv_file(&path, "userA").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "gpt-4o");
        assert_eq!(records[0].provider_id, "cursor");
        assert_eq!(records[0].input_tokens, 5);
        assert_eq!(records[0].output_tokens, 15);
        assert_eq!(records[0].cache_creation, 5);
        assert_eq!(records[0].session_id, "cursor-userA-2025-02-01");
    }

    #[test]
    fn test_parse_cursor_csv_v2() {
        let csv = r#"Date,Kind,Model,Max Mode,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost
"2025-11-13T18:36:05.846Z","Included","auto","No","28342","775","105891","21282","156290","0.19""#;
        let (_dir, path) = write_temp_csv(csv);
        let records = parse_cursor_csv_file(&path, "userB").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "auto");
        assert_eq!(records[0].input_tokens, 775);
        assert_eq!(records[0].cache_read, 105891);
        assert_eq!(records[0].cache_creation, 28342 - 775);
        assert_eq!(records[0].session_id, "cursor-userB-2025-11-13");
    }

    #[test]
    fn test_parse_cursor_csv_v3() {
        let csv = r#"Date,Cloud Agent ID,Automation ID,Kind,Model,Max Mode,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost
"2026-04-09T20:01:10.528Z","bc-a380fb49-e1a5-414e-817d-6a85b6cdc51c","cc30782e-26cc-4359-bc22-7567efe282be","Included","composer-2","Yes","0","343446","29045760","915201","30304407","Included""#;
        let (_dir, path) = write_temp_csv(csv);
        let records = parse_cursor_csv_file(&path, "u1").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "composer-2");
        assert_eq!(records[0].cache_read, 29045760);
        assert_eq!(records[0].session_id, "cursor-u1-2026-04-09");
    }

    #[test]
    fn test_session_id_differs_by_user() {
        let csv = "Date,Model,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost
2025-02-01,gpt-4o,10,5,0,15,30,$0.10";
        let (_dir, path) = write_temp_csv(csv);
        let a = parse_cursor_csv_file(&path, "accA").unwrap();
        let b = parse_cursor_csv_file(&path, "accB").unwrap();
        assert_ne!(a[0].session_id, b[0].session_id);
        assert_eq!(a[0].session_id, "cursor-accA-2025-02-01");
        assert_eq!(b[0].session_id, "cursor-accB-2025-02-01");
    }

    #[test]
    fn test_multi_account_records_survive_dedup() {
        use crate::services::dedup::dedup_records;
        let csv = "Date,Model,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost
2025-02-01,gpt-4o,10,5,0,15,30,$0.10";
        let (_dir, path) = write_temp_csv(csv);
        let mut a = parse_cursor_csv_file(&path, "accA").unwrap();
        let b = parse_cursor_csv_file(&path, "accB").unwrap();
        a.extend(b);
        let deduped = dedup_records(a);
        assert_eq!(deduped.len(), 2, "同日同模型同 token 的不同账号不应被去重");
    }

    #[test]
    fn test_detect_cursor_cache() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!detect_cursor_cache(dir.path().to_str().unwrap()));
        let path = dir.path().join("usage.csv");
        std::fs::write(&path, "Date,Model,Input (w/ Cache Write)\n").unwrap();
        assert!(detect_cursor_cache(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_preview_csv_model_filter() {
        let csv = r#"Date,Kind,Model,Max Mode,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost
"2026-07-15T10:00:00.000Z","Included","composer-2.5","No","100","50","0","20","70","0.01"
"2026-07-15T11:00:00.000Z","Included","cursor-grok-4.5-high","No","200","80","0","30","110","0.02"
"2026-07-15T12:00:00.000Z","Included","composer-2.5","No","300","90","0","40","130","0.03""#;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("usage.csv"), csv).unwrap();
        let mut source = CursorCsvService::new();
        source.open(dir.path().to_str().unwrap()).unwrap();

        let all = source.preview_csv(1, 50, false, None);
        assert_eq!(all.total, 3);
        assert_eq!(all.available_models, vec!["composer-2.5", "cursor-grok-4.5-high"]);

        let filtered = source.preview_csv(1, 50, false, Some("composer-2.5"));
        assert_eq!(filtered.total, 2);
        assert!(filtered.items.iter().all(|r| r.model == "composer-2.5"));
        assert_eq!(
            filtered.available_models,
            vec!["composer-2.5", "cursor-grok-4.5-high"]
        );
    }
}
