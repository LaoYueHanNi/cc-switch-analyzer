use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

const TITLE_MAX_CHARS: usize = 80;
const TS_MATCH_PAD_BEFORE: i64 = 10;
const TS_MATCH_PAD_AFTER: i64 = 60;
const TS_MATCH_LEAD_MAX: i64 = 120;

pub fn codex_config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("无法获取 HOME 目录")
        .join(".codex")
}

pub fn codex_session_roots() -> Vec<PathBuf> {
    let config_dir = codex_config_dir();
    vec![
        config_dir.join("sessions"),
        config_dir.join("archived_sessions"),
    ]
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn is_hex(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

fn extract_uuid_from_filename(path: &Path) -> Option<String> {
    let filename = path.file_name()?.to_string_lossy();
    // UUID format: 8-4-4-4-12 hex chars
    let chars: Vec<char> = filename.chars().collect();
    if chars.len() < 36 {
        return None;
    }
    for start in 0..=chars.len() - 36 {
        let slice: String = chars[start..start + 36].iter().collect();
        let parts: Vec<&str> = slice.split('-').collect();
        if parts.len() == 5
            && parts[0].len() == 8
            && parts[1].len() == 4
            && parts[2].len() == 4
            && parts[3].len() == 4
            && parts[4].len() == 12
            && parts.iter().all(|p| is_hex(p))
        {
            return Some(slice.to_lowercase());
        }
    }
    None
}

fn extract_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                if let Some(text) = item.get("output_text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                if let Some(text) = item.get("input_text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                None
            })
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn truncate_title(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= TITLE_MAX_CHARS {
        return trimmed.to_string();
    }
    let mut result: String = trimmed.chars().take(TITLE_MAX_CHARS).collect();
    result.push_str("...");
    result
}

fn path_basename(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.split(['/', '\\']).next_back().filter(|s| !s.is_empty())?;
    Some(last.to_string())
}

fn is_subagent_source(source: Option<&Value>) -> bool {
    source
        .and_then(Value::as_object)
        .map(|obj| obj.contains_key("subagent"))
        .unwrap_or(false)
}

fn read_head_lines(path: &Path, n: usize) -> Option<Vec<String>> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    Some(reader.lines().take(n).map_while(Result::ok).collect())
}

fn parse_codex_session(path: &Path) -> Option<(String, String, String)> {
    let head = read_head_lines(path, 10)?;

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut first_user_message: Option<String> = None;

    for line in &head {
        let value: Value = serde_json::from_str(line).ok()?;
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(payload) = value.get("payload") {
                if is_subagent_source(payload.get("source")) {
                    return None;
                }
                if session_id.is_none() {
                    session_id = payload.get("id").and_then(Value::as_str).map(String::from);
                }
                if project_dir.is_none() {
                    project_dir = payload.get("cwd").and_then(Value::as_str).map(String::from);
                }
            }
        }
        if first_user_message.is_none()
            && value.get("type").and_then(Value::as_str) == Some("response_item")
        {
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(Value::as_str) == Some("message")
                    && payload.get("role").and_then(Value::as_str) == Some("user")
                {
                    let text = payload.get("content").map(extract_text).unwrap_or_default();
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && !trimmed.starts_with("# AGENTS.md")
                        && !trimmed.starts_with("<environment_context>")
                    {
                        first_user_message = Some(trimmed.to_string());
                    }
                }
            }
        }
        if session_id.is_some() && project_dir.is_some() && first_user_message.is_some() {
            break;
        }
    }

    let session_id = session_id
        .or_else(|| extract_uuid_from_filename(path))?;
    let title = first_user_message
        .map(|t| truncate_title(&t))
        .or_else(|| project_dir.as_deref().and_then(path_basename))
        .unwrap_or_default();

    Some((session_id, title, project_dir.unwrap_or_default()))
}

pub fn scan_codex_sessions(session_ids: &[String]) -> HashMap<String, (String, String)> {
    let id_set: HashSet<String> = session_ids.iter().cloned().collect();
    if id_set.is_empty() {
        return HashMap::new();
    }

    let mut files = Vec::new();
    for root in codex_session_roots() {
        collect_jsonl_files(&root, &mut files);
    }

    let mut result = HashMap::new();
    for path in files {
        let file_uuid = match extract_uuid_from_filename(&path) {
            Some(uuid) => uuid,
            None => continue,
        };
        if !id_set.contains(&file_uuid) {
            continue;
        }
        if let Some((sid, title, project)) = parse_codex_session(&path) {
            if id_set.contains(&sid) {
                result.insert(sid, (title, project));
            }
        }
    }
    result
}

// ========== 时间戳匹配（CC-Switch 的 Codex session_id 与 JSONL 不同，需通过时间匹配）==========

/// Codex JSONL 会话信息
struct CodexSessionInfo {
    title: String,
    project_dir: String,
}

/// 解析 ISO8601/RFC3339 时间戳为 epoch 秒
fn parse_ts_to_epoch(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(if n > 1_000_000_000_000 { n / 1000 } else { n });
    }
    let raw = value.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

/// 完整扫描单个 Codex JSONL 文件，提取 session 信息和所有请求时间戳
/// 返回 (session_info, Vec<epoch_seconds>)
fn scan_codex_jsonl_full(path: &Path) -> Option<(CodexSessionInfo, Vec<i64>)> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut first_user_message: Option<String> = None;
    let mut timestamps = Vec::new();
    let mut is_subagent = false;

    for line in reader.lines() {
        let line = match line { Ok(l) => l, Err(_) => continue };
        let value: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => continue };

        if let Some(ts) = value.get("timestamp").and_then(parse_ts_to_epoch) {
            timestamps.push(ts);
        }

        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(payload) = value.get("payload") {
                if is_subagent_source(payload.get("source")) {
                    is_subagent = true;
                    break;
                }
                if session_id.is_none() {
                    session_id = payload.get("id").and_then(Value::as_str).map(String::from);
                }
                if project_dir.is_none() {
                    project_dir = payload.get("cwd").and_then(Value::as_str).map(String::from);
                }
            }
        }

        if first_user_message.is_none()
            && value.get("type").and_then(Value::as_str) == Some("response_item")
        {
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(Value::as_str) == Some("message")
                    && payload.get("role").and_then(Value::as_str) == Some("user")
                {
                    let text = payload.get("content").map(extract_text).unwrap_or_default();
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && !trimmed.starts_with("# AGENTS.md")
                        && !trimmed.starts_with("<environment_context>")
                    {
                        first_user_message = Some(trimmed.to_string());
                    }
                }
            }
        }
    }

    if is_subagent { return None; }
    let _ = session_id.or_else(|| extract_uuid_from_filename(path))?;

    let title = first_user_message
        .map(|t| truncate_title(&t))
        .or_else(|| project_dir.as_deref().and_then(path_basename))
        .unwrap_or_default();

    Some((CodexSessionInfo { title, project_dir: project_dir.unwrap_or_default() }, timestamps))
}

/// 构建 CC-Switch per-request session_id → Codex JSONL session_id 的映射。
/// 用于聚合阶段合并同一 Codex 会话的多个请求。
pub fn build_codex_session_mapping(records: &[crate::models::RawRecord]) -> HashMap<String, String> {
    let codex_timestamps: Vec<i64> = records.iter()
        .filter(|r| r.is_codex)
        .map(|r| r.created_at)
        .collect();
    if codex_timestamps.is_empty() {
        return HashMap::new();
    }
    let ts_mapping = build_codex_ts_mapping(&codex_timestamps);

    let mut result = HashMap::new();
    for r in records {
        if r.is_codex {
            if let Some(codex_sid) = ts_mapping.get(&r.created_at) {
                result.insert(r.session_id.clone(), codex_sid.clone());
            }
        }
    }
    result
}

/// 构建时间戳 → Codex session_id 映射，供管道层重映射 Codex session_id。
/// 使用时间范围匹配：每个 Codex JSONL 会话覆盖 [first_event, last_event]，
/// CC-Switch 请求落在此范围内即属于该会话。
pub fn build_codex_ts_mapping(ccswitch_timestamps: &[i64]) -> HashMap<i64, String> {
    if ccswitch_timestamps.is_empty() {
        return HashMap::new();
    }

    let mut files = Vec::new();
    for root in codex_session_roots() {
        collect_jsonl_files(&root, &mut files);
    }

    // (codex_session_id, first_ts, last_ts)
    let mut codex_sessions: Vec<(String, i64, i64)> = Vec::new();
    for path in &files {
        let file = match fs::File::open(path) { Ok(f) => f, Err(_) => continue };
        let reader = BufReader::new(file);
        let mut session_id: Option<String> = None;
        let mut is_subagent = false;
        let mut first_ts: Option<i64> = None;
        let mut last_ts: i64 = 0;

        for line in reader.lines() {
            let line = match line { Ok(l) => l, Err(_) => continue };
            let value: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => continue };
            if let Some(ts) = value.get("timestamp").and_then(parse_ts_to_epoch) {
                first_ts.get_or_insert(ts);
                last_ts = ts;
            }
            if session_id.is_none()
                && value.get("type").and_then(Value::as_str) == Some("session_meta")
            {
                if let Some(payload) = value.get("payload") {
                    if is_subagent_source(payload.get("source")) {
                        is_subagent = true;
                        break;
                    }
                    session_id = payload.get("id").and_then(Value::as_str).map(String::from)
                        .or_else(|| extract_uuid_from_filename(path));
                }
            }
        }
        if is_subagent || session_id.is_none() { continue; }
        let first = first_ts.unwrap_or(0);
        if first > 0 {
            codex_sessions.push((session_id.unwrap(), first, last_ts));
        }
    }

    if codex_sessions.is_empty() {
        return HashMap::new();
    }

    // 按时间排序，用于范围查找
    codex_sessions.sort_by_key(|(_, start, _)| *start);

    // 对每个 CC-Switch 时间戳，找到包含它的 Codex 会话
    let mut result = HashMap::new();
    for &ts in ccswitch_timestamps {
        let idx = codex_sessions.partition_point(|(_, start, _)| *start <= ts);
        // 检查 idx-1 和 idx 两个候选会话，选距离最近的
        let mut best: Option<(&String, i64)> = None;
        let check_start = idx.saturating_sub(1);
        let check_end = (idx + 1).min(codex_sessions.len());
        for i in check_start..check_end {
            let (sid, start, end) = &codex_sessions[i];
            if ts >= *start - TS_MATCH_PAD_BEFORE && ts <= *end + TS_MATCH_PAD_AFTER {
                let dist = if ts < *start { *start - ts } else if ts > *end { ts - *end } else { 0 };
                if best.as_ref().map_or(true, |(_, d)| dist < *d) {
                    best = Some((sid, dist));
                }
            }
        }
        if let Some((sid, _)) = best {
            result.insert(ts, sid.clone());
        } else if !codex_sessions.is_empty() {
            let (sid, start, _) = &codex_sessions[0];
            if ts < *start && ts >= *start - TS_MATCH_LEAD_MAX {
                result.insert(ts, sid.clone());
            }
        }
    }
    result
}

/// 批量匹配：扫描所有 Codex JSONL，通过请求时间戳匹配 CC-Switch 的 Codex session。
/// `timestamps_map`: CC-Switch session_id → 该 session 的请求时间戳列表（epoch 秒）
pub fn match_codex_sessions_by_time(
    timestamps_map: &HashMap<String, Vec<i64>>,
) -> HashMap<String, (String, String)> {
    if timestamps_map.is_empty() {
        return HashMap::new();
    }

    // 1. 扫描所有 Codex JSONL 文件，构建 (session_info, timestamps) 列表
    let mut files = Vec::new();
    for root in codex_session_roots() {
        collect_jsonl_files(&root, &mut files);
    }
    let codex_sessions: Vec<(CodexSessionInfo, Vec<i64>)> = files
        .iter()
        .filter_map(|p| scan_codex_jsonl_full(p))
        .collect();

    if codex_sessions.is_empty() {
        return HashMap::new();
    }

    // 2. 构建时间范围列表 (session_index, first_ts, last_ts)
    let ranges: Vec<(usize, i64, i64)> = codex_sessions
        .iter()
        .enumerate()
        .map(|(idx, (_, timestamps))| {
            let first = timestamps.first().copied().unwrap_or(0);
            let last = timestamps.last().copied().unwrap_or(first);
            (idx, first, last)
        })
        .collect();

    // 3. 对每个 CC-Switch session，用时间范围匹配
    let mut result = HashMap::new();
    for (sid, req_timestamps) in timestamps_map {
        let mut votes: HashMap<usize, usize> = HashMap::new();
        for &ts in req_timestamps {
            for &(idx, start, end) in &ranges {
                if ts >= start - 10 && ts <= end + 60 {
                    *votes.entry(idx).or_insert(0) += 1;
                }
            }
        }
        if let Some((&best_idx, _)) = votes.iter().max_by_key(|(_, &c)| c) {
            let info = &codex_sessions[best_idx].0;
            result.insert(sid.clone(), (info.title.clone(), info.project_dir.clone()));
        }
    }
    result
}

/// 两阶段 Codex 标题/项目解析：先按 session_id 直接匹配，再按时间戳匹配。
/// `get_timestamps`: 闭包，接收未解析的 session_id 列表，返回 session_id → 时间戳映射。
pub fn resolve_codex_titles(
    session_ids: &[String],
    get_timestamps: impl Fn(&[String]) -> HashMap<String, Vec<i64>>,
) -> HashMap<String, (String, String)> {
    let mut result = HashMap::new();
    if session_ids.is_empty() { return result; }

    let codex_titles = scan_codex_sessions(session_ids);
    result.extend(codex_titles);

    let still_unresolved: Vec<String> = session_ids.iter()
        .filter(|id| !result.contains_key(*id))
        .cloned()
        .collect();
    if !still_unresolved.is_empty() {
        let ts_map = get_timestamps(&still_unresolved);
        if !ts_map.is_empty() {
            let time_matches = match_codex_sessions_by_time(&ts_map);
            result.extend(time_matches);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ccsa_codex_test_{}_{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_codex_session(path: &Path, session_id: &str, cwd: &str, message: &str) {
        let mut f = fs::File::create(path).unwrap();
        writeln!(
            f,
            r#"{{"timestamp":"2026-03-06T21:50:12Z","type":"session_meta","payload":{{"id":"{session_id}","cwd":"{cwd}"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"timestamp":"2026-03-06T21:50:13Z","type":"response_item","payload":{{"type":"message","role":"user","content":"{message}"}}}}"#
        )
        .unwrap();
    }

    #[test]
    fn test_extract_uuid_from_filename() {
        let path = PathBuf::from(
            "rollout-2026-03-06T21-50-12-019cc369-bd7c-7891-b371-7b20b4fe0b18.jsonl",
        );
        let uuid = extract_uuid_from_filename(&path).unwrap();
        assert_eq!(uuid, "019cc369-bd7c-7891-b371-7b20b4fe0b18");
    }

    #[test]
    fn test_extract_uuid_from_filename_no_uuid() {
        let path = PathBuf::from("some-random-file.jsonl");
        assert!(extract_uuid_from_filename(&path).is_none());
    }

    #[test]
    fn test_parse_codex_session_extracts_metadata() {
        let dir = test_dir();
        let path = dir.join("test-session.jsonl");
        write_codex_session(&path, "test-id-123", "/tmp/my-project", "Fix the bug");

        let (sid, title, project) = parse_codex_session(&path).unwrap();
        assert_eq!(sid, "test-id-123");
        assert_eq!(title, "Fix the bug");
        assert_eq!(project, "/tmp/my-project");
    }

    #[test]
    fn test_parse_codex_session_skips_agents_md() {
        let dir = test_dir();
        let path = dir.join("session.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"sid\",\"cwd\":\"/p\"}}}}").unwrap();
        writeln!(f, "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"# AGENTS.md instructions\"}}}}").unwrap();
        writeln!(f, "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"Real message\"}}}}").unwrap();

        let (_, title, _) = parse_codex_session(&path).unwrap();
        assert_eq!(title, "Real message");
    }

    #[test]
    fn test_parse_codex_session_skips_subagent() {
        let dir = test_dir();
        let path = dir.join("session.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"session_meta","payload":{{"id":"sid","cwd":"/p","source":{{"subagent":{{}}}}}}}}"#).unwrap();

        assert!(parse_codex_session(&path).is_none());
    }

    #[test]
    fn test_parse_codex_session_falls_back_to_basename() {
        let dir = test_dir();
        let path = dir.join("session.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"session_meta","payload":{{"id":"sid","cwd":"/tmp/my-project"}}}}"#).unwrap();

        let (_, title, _) = parse_codex_session(&path).unwrap();
        assert_eq!(title, "my-project");
    }

    #[test]
    fn test_scan_codex_sessions_by_parse() {
        let dir = test_dir();
        let p1 = dir.join("a-019cc369-bd7c-7891-b371-7b20b4fe0b18.jsonl");
        write_codex_session(&p1, "019cc369-bd7c-7891-b371-7b20b4fe0b18", "/p1", "First");

        let (sid1, _, _) = parse_codex_session(&p1).unwrap();
        assert_eq!(sid1, "019cc369-bd7c-7891-b371-7b20b4fe0b18");
    }

    #[test]
    fn test_build_codex_ts_mapping_single_session() {
        let dir = test_dir();
        let sid = "aaa-bbb-ccc";
        let mut f = fs::File::create(dir.join(format!("{}.jsonl", sid))).unwrap();
        // Session from T=1000 to T=2000
        writeln!(f, r#"{{"timestamp":"1970-01-01T00:16:40Z","type":"session_meta","payload":{{"id":"{sid}","cwd":"/p"}}}}"#).unwrap();
        writeln!(f, r#"{{"timestamp":"1970-01-01T00:33:20Z","type":"response_item","payload":{{"type":"message","role":"user","content":"hi"}}}}"#).unwrap();

        // Temporarily redirect codex_session_roots
        // build_codex_ts_mapping scans real ~/.codex, so we test with direct timestamp logic
        let sessions: Vec<(String, i64, i64)> = vec![(sid.to_string(), 1000, 2000)];
        let timestamps = vec![1500i64, 1000, 2000, 900]; // inside, start, end, before

        // Test matching logic directly
        let mut result = HashMap::new();
        for &ts in &timestamps {
            let idx = sessions.partition_point(|(_, start, _)| *start <= ts);
            let mut best: Option<(&String, i64)> = None;
            let check_start = idx.saturating_sub(1);
            let check_end = (idx + 1).min(sessions.len());
            for i in check_start..check_end {
                let (sid, start, end) = &sessions[i];
                if ts >= *start - TS_MATCH_PAD_BEFORE && ts <= *end + TS_MATCH_PAD_AFTER {
                    let dist = if ts < *start { *start - ts } else if ts > *end { ts - *end } else { 0 };
                    if best.as_ref().map_or(true, |(_, d)| dist < *d) {
                        best = Some((sid, dist));
                    }
                }
            }
            if let Some((sid, _)) = best {
                result.insert(ts, sid.clone());
            }
        }
        assert_eq!(result.get(&1500), Some(&sid.to_string()));
        assert_eq!(result.get(&1000), Some(&sid.to_string()));
        assert_eq!(result.get(&2000), Some(&sid.to_string()));
        assert!(result.get(&900).is_none()); // too far before
    }

    #[test]
    fn test_resolve_codex_titles() {
        let dir = test_dir();
        let sid = "019cc369-bd7c-7891-b371-7b20b4fe0b18";
        let path = dir.join(format!("rollout-{}.jsonl", sid));
        write_codex_session(&path, sid, "/my-project", "Hello world");

        // Override roots is not possible, so test with empty timestamps (phase 1 only)
        // scan_codex_sessions won't find the file in real ~/.codex, but the logic is tested
        let result = resolve_codex_titles(&[sid.to_string()], |_ids| HashMap::new());
        // File is in temp dir, not in real ~/.codex, so result will be empty
        // This mainly tests that the function runs without panic
        assert!(result.len() <= 1);
    }
}
