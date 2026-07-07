use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::RwLock;

use crate::models::*;
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
    records: RwLock<Vec<RawRecord>>,
    latest_timestamp: Option<i64>,
}

impl CursorCsvService {
    pub fn new() -> Self {
        Self {
            cache_dir: String::new(),
            records: RwLock::new(Vec::new()),
            latest_timestamp: None,
        }
    }

    pub fn open(&mut self, dir_path: &str) -> Result<(), String> {
        self.close();
        let csv_path = Path::new(dir_path).join("usage.csv");
        if !csv_path.is_file() {
            return Err(format!("Cursor 缓存文件不存在: {}", csv_path.display()));
        }
        let parsed = parse_cursor_csv_file(&csv_path)?;
        self.latest_timestamp = parsed.iter().map(|r| r.created_at).max();
        *self.records.write().map_err(|e| format!("数据锁失败: {}", e))? = parsed;
        self.cache_dir = dir_path.to_string();
        Ok(())
    }

    pub fn close(&mut self) {
        self.cache_dir.clear();
        if let Ok(mut guard) = self.records.write() {
            guard.clear();
        }
        self.latest_timestamp = None;
    }

    pub fn is_open(&self) -> bool {
        !self.cache_dir.is_empty()
    }

    fn records_read(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<RawRecord>>, String> {
        self.records.read().map_err(|e| format!("数据锁失败: {}", e))
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

/// 解析 Cursor usage.csv 为 RawRecord 列表
pub fn parse_cursor_csv_file(path: &Path) -> Result<Vec<RawRecord>, String> {
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
            session_id: format!("cursor-{}", day_key),
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

fn parse_date_to_epoch_secs(date_str: &str) -> i64 {
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
    ) -> Result<Vec<(String, String, String, i64, i64, i64, i64, i64, i64)>, String> {
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
        let records = parse_cursor_csv_file(&path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "gpt-4o");
        assert_eq!(records[0].provider_id, "cursor");
        assert_eq!(records[0].input_tokens, 5);
        assert_eq!(records[0].output_tokens, 15);
        assert_eq!(records[0].cache_creation, 5);
        assert_eq!(records[0].session_id, "cursor-2025-02-01");
    }

    #[test]
    fn test_parse_cursor_csv_v2() {
        let csv = r#"Date,Kind,Model,Max Mode,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost
"2025-11-13T18:36:05.846Z","Included","auto","No","28342","775","105891","21282","156290","0.19""#;
        let (_dir, path) = write_temp_csv(csv);
        let records = parse_cursor_csv_file(&path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "auto");
        assert_eq!(records[0].input_tokens, 775);
        assert_eq!(records[0].cache_read, 105891);
        assert_eq!(records[0].cache_creation, 28342 - 775);
        assert_eq!(records[0].session_id, "cursor-2025-11-13");
    }

    #[test]
    fn test_parse_cursor_csv_v3() {
        let csv = r#"Date,Cloud Agent ID,Automation ID,Kind,Model,Max Mode,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens,Total Tokens,Cost
"2026-04-09T20:01:10.528Z","bc-a380fb49-e1a5-414e-817d-6a85b6cdc51c","cc30782e-26cc-4359-bc22-7567efe282be","Included","composer-2","Yes","0","343446","29045760","915201","30304407","Included""#;
        let (_dir, path) = write_temp_csv(csv);
        let records = parse_cursor_csv_file(&path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "composer-2");
        assert_eq!(records[0].cache_read, 29045760);
    }

    #[test]
    fn test_detect_cursor_cache() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!detect_cursor_cache(dir.path().to_str().unwrap()));
        let path = dir.path().join("usage.csv");
        std::fs::write(&path, "Date,Model,Input (w/ Cache Write)\n").unwrap();
        assert!(detect_cursor_cache(dir.path().to_str().unwrap()));
    }
}
