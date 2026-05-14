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

fn claude_projects_dir() -> PathBuf {
    dirs::home_dir().expect("无法获取 HOME 目录").join(".claude").join("projects")
}

pub fn short_session(session_id: &str) -> String {
    if session_id.starts_with("ses_") {
        session_id[..8.min(session_id.len())].to_string()
    } else {
        session_id.split('-').next().unwrap_or(&session_id[..8.min(session_id.len())]).to_string()
    }
}

/// 返回 (jsonl路径, 项目文件夹名)
fn find_jsonl(session_id: &str) -> Option<(PathBuf, String)> {
    let dir = claude_projects_dir();
    if !dir.exists() { return None; }
    let target = format!("{}.jsonl", session_id);
    for entry in fs::read_dir(&dir).ok()? {
        let entry = entry.ok()?;
        if entry.path().is_dir() {
            let candidate = entry.path().join(&target);
            if candidate.exists() {
                let project_name = entry.file_name().to_string_lossy().to_string();
                return Some((candidate, project_name));
            }
        }
    }
    None
}

fn extract_first_user_message(path: &PathBuf) -> Option<String> {
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

fn clean_title(text: &str) -> String {
    text.trim().trim_start_matches('/').split('\n').next().unwrap_or("").trim().to_string()
}

/// 返回 (标题, 项目名)
pub fn generate_title(session_id: &str) -> Option<(String, String)> {
    let (path, project_name) = find_jsonl(session_id)?;
    let message = extract_first_user_message(&path)?;
    let title = clean_title(&message);
    if title.is_empty() { None } else { Some((title, project_name)) }
}
