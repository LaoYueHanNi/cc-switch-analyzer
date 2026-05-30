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

/// 返回 jsonl 路径
fn find_jsonl(session_id: &str) -> Option<PathBuf> {
    let dir = claude_projects_dir();
    if !dir.exists() { return None; }
    let target = format!("{}.jsonl", session_id);
    for entry in fs::read_dir(&dir).ok()? {
        let entry = entry.ok()?;
        if entry.path().is_dir() {
            let candidate = entry.path().join(&target);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 一次遍历目录，批量匹配多个 session_id 的 jsonl 路径
pub fn find_jsonl_batch(session_ids: &[String]) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let dir = claude_projects_dir();
    if !dir.exists() { return result; }
    let remaining: std::collections::HashSet<String> = session_ids.iter().cloned().collect();

    for entry in fs::read_dir(&dir).ok().into_iter().flatten() {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        if !entry.path().is_dir() { continue; }
        for file_entry in fs::read_dir(entry.path()).ok().into_iter().flatten() {
            let file_entry = match file_entry { Ok(e) => e, Err(_) => continue };
            let path = file_entry.path();
            if path.extension().map_or(true, |e| e != "jsonl") { continue; }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if remaining.contains(stem) {
                    result.insert(stem.to_string(), path.to_string_lossy().to_string());
                }
            }
        }
    }
    result
}

/// 一次遍历 JSONL 提取：cwd、ai-title、第一条 user message
/// 三个字段找到后提前终止，避免重复打开文件
fn extract_session_metadata(path: &PathBuf) -> (Option<String>, Option<String>, Option<String>) {
    use std::io::BufRead;
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None, None),
    };
    let reader = std::io::BufReader::new(file);
    let mut cwd: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut user_message: Option<String> = None;

    for line in reader.lines() {
        let line = match line { Ok(l) => l, Err(_) => continue };
        if cwd.is_some() && ai_title.is_some() && user_message.is_some() { break; }
        let obj: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if cwd.is_none() {
            if let Some(v) = obj.get("cwd").and_then(|v| v.as_str()) {
                let v = v.trim().to_string();
                if !v.is_empty() { cwd = Some(v); }
            }
        }

        let typ = obj.get("type").and_then(|v| v.as_str());
        if ai_title.is_none() && typ == Some("ai-title") {
            if let Some(v) = obj.get("aiTitle").and_then(|v| v.as_str()) {
                let v = v.trim().to_string();
                if !v.is_empty() { ai_title = Some(v); }
            }
        }

        if user_message.is_none() && typ == Some("user") {
            if let Some(msg) = obj.get("message").and_then(|m| m.get("content")) {
                if let Some(text) = extract_text(msg) {
                    user_message = Some(clean_title(&text));
                }
            }
        }
    }
    (cwd, ai_title, user_message)
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
/// 优先从 ai-title 记录获取，fallback 到第一条 user message
pub fn generate_title(session_id: &str) -> Option<(String, String)> {
    let path = find_jsonl(session_id)?;
    let (cwd, ai_title, user_message) = extract_session_metadata(&path);
    let title = ai_title.or(user_message).filter(|t| !t.is_empty())?;
    Some((title, cwd.unwrap_or_default()))
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
        .filter(|id| !cached.contains_key(*id))
        .cloned()
        .collect();
    if uncached.is_empty() {
        return Ok((project_map, title_map));
    }

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

    // 3. Codex JSONL 兜底（先按 session_id 匹配，再按时间戳匹配）
    let remaining_for_codex: Vec<String> = uncached.iter()
        .filter(|id| !resolved.contains_key(*id))
        .cloned()
        .collect();
    if !remaining_for_codex.is_empty() {
        let codex_resolved = crate::services::codex_sessions::resolve_codex_titles(
            &remaining_for_codex,
            |ids| {
                let mut map: HashMap<String, Vec<i64>> = HashMap::new();
                for entry in data_sources.iter() {
                    if let Ok(timestamps) = entry.source.get_session_timestamps(ids) {
                        for (sid, times) in timestamps {
                            map.entry(sid).or_default().extend(times);
                        }
                    }
                }
                map
            },
        );
        for (id, (title, project)) in codex_resolved {
            resolved.insert(id, (title, project, "codex".to_string()));
        }
    }

    // 4. Claude JSONL 兜底
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

    // 5. 合并结果 + 写入 sessions 表（包括未找到的，标记 not_found 避免重复扫描）
    for id in &uncached {
        if let Some((title, project, source)) = resolved.remove(id) {
            if !title.is_empty() { title_map.insert(id.clone(), title.clone()); }
            if !project.is_empty() { project_map.insert(id.clone(), project.clone()); }
            app_db.save_session(id, &project, &title, &source)?;
            app_db.save_session_title(id, &format!("{}|{}", title, project), &source)?;
        } else {
            app_db.save_session(id, "", "", "not_found")?;
        }
    }

    Ok((project_map, title_map))
}

/// 通过 sessionId 查找 JSONL 文件路径
pub fn find_jsonl_path(session_id: &str) -> Option<String> {
    find_jsonl(session_id).map(|path| path.to_string_lossy().to_string())
}
