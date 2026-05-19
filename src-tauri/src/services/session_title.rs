use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ========== 可扩展的标题解析接口 ==========

/// 标题解析接口，不同来源可以实现此 trait
pub trait TitleProvider: Send + Sync {
    /// 批量获取会话标题。返回能解析的 session_id -> (title, project)
    fn get_titles(&self, session_ids: &[String]) -> HashMap<String, (String, String)>;
}

/// 从 Claude 本地 JSONL 文件提取标题
pub struct ClaudeJsonlTitleProvider;

impl TitleProvider for ClaudeJsonlTitleProvider {
    fn get_titles(&self, session_ids: &[String]) -> HashMap<String, (String, String)> {
        let mut result = HashMap::new();
        for id in session_ids {
            if let Some((title, project)) = generate_title(id) {
                result.insert(id.clone(), (title, project));
            }
        }
        result
    }
}

// ========== Claude JSONL 文件解析 ==========

pub fn claude_projects_dir() -> PathBuf {
    dirs::home_dir().expect("无法获取 HOME 目录").join(".claude").join("projects")
}

pub fn short_session(session_id: &str) -> String {
    if session_id.starts_with("ses_") {
        session_id[..8.min(session_id.len())].to_string()
    } else {
        session_id.split('-').next().unwrap_or(&session_id[..8.min(session_id.len())]).to_string()
    }
}

/// 返回 (jsonl路径, cwd 工作目录)
fn find_jsonl(session_id: &str) -> Option<(PathBuf, Option<String>)> {
    let dir = claude_projects_dir();
    if !dir.exists() { return None; }
    let target = format!("{}.jsonl", session_id);
    for entry in fs::read_dir(&dir).ok()? {
        let entry = entry.ok()?;
        if entry.path().is_dir() {
            let candidate = entry.path().join(&target);
            if candidate.exists() {
                let cwd = extract_cwd(&candidate);
                return Some((candidate, cwd));
            }
        }
    }
    None
}

/// 从 JSONL 文件前 10 行提取 cwd 字段
fn extract_cwd(path: &PathBuf) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines().take(10) {
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(cwd) = obj.get("cwd").and_then(|v| v.as_str()) {
                let cwd = cwd.trim().to_string();
                if !cwd.is_empty() { return Some(cwd); }
            }
        }
    }
    None
}

pub fn extract_first_user_message(path: &PathBuf) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
            if obj.get("type").and_then(|v| v.as_str()) != Some("user") { continue; }
            if let Some(msg) = obj.get("message").and_then(|m| m.get("content")) {
                if let Some(text) = extract_text(msg) {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn extract_text(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        serde_json::Value::Array(arr) => {
            let text: String = arr.iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                        b.get("text").and_then(|v| v.as_str()).map(String::from)
                    } else { None }
                })
                .collect::<Vec<_>>()
                .join(" ");
            if text.trim().is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}

pub fn clean_title(text: &str) -> String {
    text.trim().trim_start_matches('/').split('\n').next().unwrap_or("").trim().to_string()
}

/// 返回 (标题, cwd 工作目录)
pub fn generate_title(session_id: &str) -> Option<(String, String)> {
    let (path, cwd) = find_jsonl(session_id)?;
    let message = extract_first_user_message(&path)?;
    let title = clean_title(&message);
    if title.is_empty() { None } else { Some((title, cwd.unwrap_or_default())) }
}

/// 通过 sessionId 批量获取项目目录和标题。
/// 流程: sessions 表 → provider（OpenCode: session.directory）→ JSONL 兜底 → 写回 sessions 表
/// 返回 (project_dir_map, title_map)
pub fn resolve_session_projects(
    session_ids: &[String],
    app_db: &crate::services::app_db::AppDbService,
    data_sources: &[crate::services::data_source::SourceEntry],
) -> Result<(HashMap<String, String>, HashMap<String, String>), String> {
    let mut project_map: HashMap<String, String> = HashMap::new();
    let mut title_map: HashMap<String, String> = HashMap::new();

    // 1. 查 sessions 表
    let cached = app_db.get_sessions(session_ids)?;
    for (sid, (project_dir, title, _source)) in &cached {
        if !title.is_empty() { title_map.insert(sid.clone(), title.clone()); }
        if !project_dir.is_empty() { project_map.insert(sid.clone(), project_dir.clone()); }
    }

    let uncached: Vec<String> = session_ids.iter()
        .filter(|id| !project_map.contains_key(*id))
        .cloned()
        .collect();
    if uncached.is_empty() { return Ok((project_map, title_map)); }

    // 2. 从各数据源获取 (OpenCode: session.directory)
    let mut resolved: HashMap<String, (String, String, String)> = HashMap::new();
    for source_entry in data_sources.iter() {
        let remaining: Vec<String> = uncached.iter()
            .filter(|id| !resolved.contains_key(*id))
            .cloned()
            .collect();
        if remaining.is_empty() { break; }
        let tag = source_entry.source.title_source_tag().unwrap_or("");
        if let Some(titles_result) = source_entry.source.get_session_titles_from_provider(&remaining) {
            if let Ok(titles) = titles_result {
                for (id, (title, project)) in titles {
                    resolved.insert(id, (title, project, tag.to_string()));
                }
            }
        }
    }

    // 3. JSONL 兜底 (Claude Code)
    let remaining_for_jsonl: Vec<String> = uncached.iter()
        .filter(|id| !resolved.contains_key(*id))
        .cloned()
        .collect();
    if !remaining_for_jsonl.is_empty() {
        for id in &remaining_for_jsonl {
            if let Some((title, project)) = generate_title(id) {
                resolved.insert(id.clone(), (title, project, "claudecode".to_string()));
            }
        }
    }

    // 4. 合并结果 + 写入 sessions 表 + 写入 session_titles 表（兼容）
    for id in &uncached {
        if let Some((title, project, source)) = resolved.remove(id) {
            if !title.is_empty() { title_map.insert(id.clone(), title.clone()); }
            if !project.is_empty() { project_map.insert(id.clone(), project.clone()); }
            app_db.save_session(id, &project, &title, &source)?;
            app_db.save_session_title(id, &format!("{}|{}", title, project), &source)?;
        }
    }

    Ok((project_map, title_map))
}

/// 通过 sessionId 查找 JSONL 文件路径
pub fn find_jsonl_path(session_id: &str) -> Option<String> {
    find_jsonl(session_id).map(|(path, _)| path.to_string_lossy().to_string())
}
