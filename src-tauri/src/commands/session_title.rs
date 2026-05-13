use std::collections::HashMap;
use serde::Serialize;
use tauri::State;

use crate::AppState;
use crate::services::session_title::{generate_title, short_session};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTitleInfo {
    title: String,
    project: String,
}

#[tauri::command]
pub fn get_session_titles(
    session_ids: Vec<String>,
    state: State<AppState>,
) -> Result<HashMap<String, SessionTitleInfo>, String> {
    let mut result = HashMap::new();

    let uncached = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let cached = app_db.get_session_titles(&session_ids)?;
        // cached 里 title 格式: "title|project" 或纯 title（旧缓存）
        for (sid, raw) in &cached {
            let parts: Vec<&str> = raw.splitn(2, '|').collect();
            result.insert(sid.clone(), SessionTitleInfo {
                title: parts[0].to_string(),
                project: parts.get(1).unwrap_or(&"").to_string(),
            });
        }
        session_ids.iter().filter(|id| !cached.contains_key(*id)).cloned().collect::<Vec<String>>()
    };

    if uncached.is_empty() { return Ok(result); }

    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    for id in &uncached {
        let (title, project) = generate_title(id)
            .unwrap_or_else(|| (short_session(id), String::new()));
        // 存储格式: "title|project"
        app_db.save_session_title(id, &format!("{}|{}", title, project))?;
        result.insert(id.clone(), SessionTitleInfo { title, project });
    }

    Ok(result)
}
