//! Grok Build 本地会话元数据（只读）
//!
//! 从 `~/.grok/{sessions,archived_sessions}/<enc-cwd>/<session-id>/summary.json`
//! 提取标题与项目路径，供 Session / Realtime / Tasks enrichment 使用。
//! 不读取 updates.jsonl，不做用量统计。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const TITLE_MAX_CHARS: usize = 80;

#[derive(Debug, Deserialize)]
struct GrokSessionInfo {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GrokSessionSummary {
    info: GrokSessionInfo,
    #[serde(default)]
    session_summary: Option<String>,
    #[serde(default)]
    generated_title: Option<String>,
}

/// `$GROK_HOME` 或 `~/.grok`
pub fn grok_home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("GROK_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".grok")
}

fn session_roots() -> Vec<PathBuf> {
    let home = grok_home_dir();
    vec![
        home.join("sessions"),
        home.join("archived_sessions"),
    ]
}

fn truncate_title(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn parse_summary(path: &Path) -> Option<(String, String, String)> {
    let text = fs::read_to_string(path).ok()?;
    let summary: GrokSessionSummary = serde_json::from_str(&text).ok()?;
    let session_id = summary
        .info
        .id
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })?;
    let title = summary
        .generated_title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            summary
                .session_summary
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .map(|s| truncate_title(s, TITLE_MAX_CHARS))
        .unwrap_or_default();
    let project = summary
        .info
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    if title.is_empty() && project.is_empty() {
        return None;
    }
    Some((session_id, title, project))
}

/// 在 sessions / archived_sessions 下按目录名匹配 session_id，读取 summary.json。
pub fn resolve_grok_titles(session_ids: &[String]) -> HashMap<String, (String, String)> {
    let mut result = HashMap::new();
    if session_ids.is_empty() {
        return result;
    }
    let mut remaining: HashSet<&str> = session_ids.iter().map(|s| s.as_str()).collect();

    for root in session_roots() {
        if remaining.is_empty() || !root.is_dir() {
            continue;
        }
        let Ok(cwd_entries) = fs::read_dir(&root) else {
            continue;
        };
        for cwd_entry in cwd_entries.flatten() {
            let cwd_path = cwd_entry.path();
            if !cwd_path.is_dir() {
                continue;
            }
            let Ok(session_entries) = fs::read_dir(&cwd_path) else {
                continue;
            };
            for session_entry in session_entries.flatten() {
                let session_path = session_entry.path();
                if !session_path.is_dir() {
                    continue;
                }
                let Some(name) = session_path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !remaining.contains(name) {
                    continue;
                }
                let summary_path = session_path.join("summary.json");
                if let Some((id, title, project)) = parse_summary(&summary_path) {
                    remaining.remove(id.as_str());
                    remaining.remove(name);
                    result.insert(id, (title, project));
                }
                if remaining.is_empty() {
                    return result;
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parse_summary_prefers_generated_title_and_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "019f6af2-18b0-7673-958e-d25be650e172";
        let session_dir = dir.path().join("encoded").join(sid);
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("summary.json"),
            format!(
                r#"{{"info":{{"id":"{sid}","cwd":"C:/work/demo"}},"session_summary":"fallback","generated_title":"Grok session"}}"#
            ),
        )
        .unwrap();

        let (id, title, project) = parse_summary(&session_dir.join("summary.json")).unwrap();
        assert_eq!(id, sid);
        assert_eq!(title, "Grok session");
        assert_eq!(project, "C:/work/demo");
    }

    #[test]
    fn resolve_grok_titles_from_env_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sid = "019f9c91-875c-7881-bef8-d4d62236f397";
        let session_dir = dir
            .path()
            .join("sessions")
            .join("C%3A%5CUsers")
            .join(sid);
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("summary.json"),
            format!(
                r#"{{"info":{{"id":"{sid}","cwd":"D:\\Code\\demo"}},"generated_title":"Local Grok"}}"#
            ),
        )
        .unwrap();

        std::env::set_var("GROK_HOME", dir.path());
        let titles = resolve_grok_titles(&[sid.to_string()]);
        std::env::remove_var("GROK_HOME");

        let (title, project) = titles.get(sid).expect("resolved");
        assert_eq!(title, "Local Grok");
        assert_eq!(project, "D:\\Code\\demo");
    }
}
