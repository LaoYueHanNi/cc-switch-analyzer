use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::fs;
use std::path::Path;

use crate::services::session_title::{claude_projects_dir, find_jsonl_path};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchProvider {
    pub id: String,
    pub name: String,
    pub has_env: bool,
}

/// 从 CC-Switch 数据库读取 Claude 供应商列表
#[tauri::command]
pub fn get_ccswitch_providers(db_path: String) -> Result<Vec<CcSwitchProvider>, String> {
    let conn = Connection::open_with_flags(
        Path::new(&db_path),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("打开 CC-Switch 数据库失败: {}", e))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, name, settings_config FROM providers
             WHERE app_type = 'claude'
             ORDER BY COALESCE(sort_index, 999999), name",
        )
        .map_err(|e| format!("查询供应商失败: {}", e))?;

    let providers = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let config_str: String = row.get(2)?;
            let has_env = serde_json::from_str::<serde_json::Value>(&config_str)
                .ok()
                .and_then(|v| v.get("env").cloned())
                .map(|env| {
                    env.as_object()
                        .map(|obj| !obj.is_empty())
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            Ok(CcSwitchProvider { id, name, has_env })
        })
        .map_err(|e| format!("读取供应商失败: {}", e))?;

    let mut result = Vec::new();
    for p in providers {
        result.push(p.map_err(|e| format!("读取供应商行失败: {}", e))?);
    }
    Ok(result)
}

/// 从 CC-Switch 数据库中提取指定供应商的 env 配置并写入临时文件
fn prepare_settings_file(provider_id: &str, db_path: &str) -> Result<String, String> {
    let conn = Connection::open_with_flags(
        Path::new(db_path),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("打开 CC-Switch 数据库失败: {}", e))?;

    let config_str: String = conn
        .query_row(
            "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = 'claude'",
            [provider_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("供应商 {} 不存在: {}", provider_id, e))?;

    let config: serde_json::Value =
        serde_json::from_str(&config_str).map_err(|e| format!("解析配置失败: {}", e))?;

    let settings_json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("序列化配置失败: {}", e))?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let temp_file = std::env::temp_dir().join(format!("ccsa_{}_{}.json", provider_id, timestamp));

    fs::write(&temp_file, settings_json)
        .map_err(|e| format!("写入临时配置文件失败: {}", e))?;

    Ok(temp_file.to_string_lossy().to_string())
}

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

/// 携带供应商配置打开 Claude 终端
#[tauri::command]
pub fn open_claude_terminal_with_provider(
    project_dir: String,
    provider_id: String,
    db_path: String,
) -> Result<(), String> {
    let settings_file = prepare_settings_file(&provider_id, &db_path)?;

    #[cfg(target_os = "windows")]
    {
        let claude_cmd = format!("claude --settings \"{}\"", settings_file);
        std::process::Command::new("wt")
            .args(["-w", "0", "-d", &project_dir, "powershell", "-NoExit", "-Command", &claude_cmd])
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

/// 携带供应商配置恢复 Claude 会话
#[tauri::command]
pub fn resume_claude_session_with_provider(
    session_id: String,
    project_dir: Option<String>,
    provider_id: String,
    db_path: String,
) -> Result<(), String> {
    let settings_file = prepare_settings_file(&provider_id, &db_path)?;

    #[cfg(target_os = "windows")]
    {
        let claude_cmd = format!("claude --resume {} --settings \"{}\"", session_id, settings_file);
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
