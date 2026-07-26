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

    // 1. 查 app_db 缓存（旧格式 project 视为未命中，强制重新解析以获取真实 cwd）
    //    grokbuild / 已知 source：有 title 且（有合法 project 或 source 已知）即可命中，避免反复扫盘
    let uncached = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let cached = app_db.get_session_titles(&session_ids)?;
        for (sid, (raw, source)) in &cached {
            let parts: Vec<&str> = raw.splitn(2, '|').collect();
            let title = parts[0].to_string();
            let project = parts.get(1).unwrap_or(&"").to_string();
            let project_valid = !project.is_empty()
                && (project.contains('/') || project.contains('\\'));
            let known_source = matches!(
                source.as_str(),
                "claudecode" | "opencode" | "codex" | "grokbuild"
            );
            if project_valid || (known_source && !title.is_empty()) {
                result.insert(sid.clone(), SessionTitleInfo {
                    title, project, source: source.clone(),
                });
            }
        }
        session_ids.iter().filter(|id| !result.contains_key(*id)).cloned().collect::<Vec<String>>()
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

    // 3. Grok Build summary.json（精确目录匹配，须在 Codex 时间戳匹配之前）
    let remaining_for_grok: Vec<String> = uncached.iter()
        .filter(|id| !resolved.contains_key(*id))
        .cloned()
        .collect();
    if !remaining_for_grok.is_empty() {
        for (id, (title, project)) in crate::services::grok_sessions::resolve_grok_titles(&remaining_for_grok) {
            resolved.insert(id, (title, project, "grokbuild".to_string()));
        }
    }

    // 4. Codex JSONL 兜底（先按 session_id 匹配，再按时间戳匹配）
    let remaining_for_codex: Vec<String> = uncached.iter()
        .filter(|id| !resolved.contains_key(*id))
        .cloned()
        .collect();
    if !remaining_for_codex.is_empty() {
        let codex_resolved = {
            let data_sources = state.data_sources.read().map_err(|e| e.to_string())?;
            crate::services::codex_sessions::resolve_codex_titles(
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
            )
        };
        for (id, (title, project)) in codex_resolved {
            resolved.insert(id, (title, project, "codex".to_string()));
        }
    }

    // 5. Claude JSONL 兜底
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

    // 6. 缓存写入 + short_session 最终兜底
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    for id in &uncached {
        let (title, project, source) = resolved.remove(id)
            .unwrap_or_else(|| (short_session(id), String::new(), String::new()));
        app_db.save_session_title(id, &format!("{}|{}", title, project), &source)?;
        result.insert(id.clone(), SessionTitleInfo { title, project, source });
    }

    Ok(result)
}
