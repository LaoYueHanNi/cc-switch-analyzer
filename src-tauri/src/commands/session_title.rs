use std::collections::HashMap;
use serde::Serialize;
use tauri::State;

use crate::AppState;
use crate::services::session_title::{ClaudeJsonlTitleProvider, TitleProvider, short_session};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTitleInfo {
    title: String,
    project: String,
    source: String,
}

#[tauri::command]
pub fn get_session_titles(
    session_ids: Vec<String>,
    state: State<AppState>,
) -> Result<HashMap<String, SessionTitleInfo>, String> {
    let mut result = HashMap::new();

    // 1. 查 app_db 缓存
    let uncached = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let cached = app_db.get_session_titles(&session_ids)?;
        for (sid, (raw, source)) in &cached {
            let parts: Vec<&str> = raw.splitn(2, '|').collect();
            result.insert(sid.clone(), SessionTitleInfo {
                title: parts[0].to_string(),
                project: parts.get(1).unwrap_or(&"").to_string(),
                source: source.clone(),
            });
        }
        session_ids.iter().filter(|id| !cached.contains_key(*id)).cloned().collect::<Vec<String>>()
    };

    if uncached.is_empty() { return Ok(result); }

    // 2. 从各数据源获取标题 (title, project, source_tag)
    let mut resolved: HashMap<String, (String, String, String)> = HashMap::new();
    {
        let data_sources = state.data_sources.read().map_err(|e| e.to_string())?;
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
    }

    // 3. JSONL 兜底
    let jsonl_provider = ClaudeJsonlTitleProvider;
    let remaining_for_jsonl: Vec<String> = uncached.iter()
        .filter(|id| !resolved.contains_key(*id))
        .cloned()
        .collect();
    if !remaining_for_jsonl.is_empty() {
        let jsonl_titles = jsonl_provider.get_titles(&remaining_for_jsonl);
        for (id, (title, project)) in jsonl_titles {
            resolved.insert(id, (title, project, "claudecode".to_string()));
        }
    }

    // 4. 缓存写入 + short_session 最终兜底
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    for id in &uncached {
        let (title, project, source) = resolved.remove(id)
            .unwrap_or_else(|| (short_session(id), String::new(), String::new()));
        app_db.save_session_title(id, &format!("{}|{}", title, project), &source)?;
        result.insert(id.clone(), SessionTitleInfo { title, project, source });
    }

    Ok(result)
}
