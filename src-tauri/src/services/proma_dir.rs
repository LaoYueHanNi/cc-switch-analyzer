//! Proma 数据目录探测
//!
//! Proma 数据源已改为「扫描入库」模式(见 proma_scanner.rs / proma_db.rs)，
//! 本模块仅保留目录探测逻辑，供 auto-load 判断 `~/.proma` 是否存在有效数据。

use std::path::Path;

/// 判定目录是否为 Proma 数据目录：存在 `agent-sessions/`（含 jsonl）
/// 或 `agent-sessions.json` 索引文件。
pub fn detect_proma_dir(path: &str) -> bool {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return false;
    }
    if dir.join("agent-sessions.json").is_file() {
        return true;
    }
    let sessions_dir = dir.join("agent-sessions");
    if !sessions_dir.is_dir() {
        return false;
    }
    std::fs::read_dir(&sessions_dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    == Some("jsonl")
            })
        })
        .unwrap_or(false)
}
