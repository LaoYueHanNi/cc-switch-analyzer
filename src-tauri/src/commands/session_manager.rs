use std::fs;
use std::path::Path;

use crate::services::session_title::{claude_projects_dir, find_jsonl_path};

#[tauri::command]
pub fn open_claude_terminal(project_dir: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("wt")
            .args(["-w", "0", "-d", &project_dir, "powershell", "-NoExit", "-Command", "claude"])
            .spawn()
            .map_err(|e| format!("启动终端失败: {}", e))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = project_dir;
        return Err("当前仅支持 Windows".to_string());
    }

    Ok(())
}

#[tauri::command]
pub fn resume_claude_session(session_id: String, project_dir: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let claude_cmd = format!("claude --resume {}", session_id);
        let mut cmd = std::process::Command::new("wt");
        cmd.arg("-w").arg("0");
        if let Some(dir) = &project_dir {
            if !dir.is_empty() {
                cmd.arg("-d").arg(dir);
            }
        }
        cmd.args(["powershell", "-NoExit", "-Command", &claude_cmd])
            .spawn()
            .map_err(|e| format!("启动终端失败: {}", e))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = session_id;
        let _ = project_dir;
        return Err("当前仅支持 Windows".to_string());
    }

    Ok(())
}

#[tauri::command]
pub fn delete_claude_session(session_id: String) -> Result<bool, String> {
    let source_path = find_jsonl_path(&session_id)
        .ok_or_else(|| format!("未找到会话 {} 的 JSONL 文件", session_id))?;
    let path = Path::new(&source_path);
    let root = claude_projects_dir();

    let canonical_root = fs::canonicalize(&root).map_err(|e| format!("无法解析根目录: {e}"))?;
    let canonical_source = fs::canonicalize(path).map_err(|e| format!("无法解析会话路径: {e}"))?;
    if !canonical_source.starts_with(&canonical_root) {
        return Err("会话文件不在允许的目录范围内".to_string());
    }

    // 删除旁挂目录
    if let Some(stem) = path.file_stem() {
        let sibling = path.parent().unwrap_or(Path::new(".")).join(stem);
        if sibling.exists() && sibling.is_dir() {
            fs::remove_dir_all(&sibling).map_err(|e| format!("删除旁挂目录失败: {e}"))?;
        }
    }

    fs::remove_file(path).map_err(|e| format!("删除会话文件失败: {e}"))?;
    Ok(true)
}
