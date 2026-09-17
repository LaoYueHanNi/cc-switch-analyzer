//! Kimi (Kimi CLI / Kimi Code / Kimi Work) 本地会话用量扫描入库
//!
//! 数据源位置支持：
//! - **Kimi Work (kimi-desktop)**:
//!   - macOS: `~/Library/Application Support/kimi-desktop/daimon-share/daimon/runtime/kimi-code/home/sessions/`
//!   - Windows: `%APPDATA%/kimi-desktop/daimon-share/daimon/runtime/kimi-code/home/sessions/`
//!     （支持读取 `%APPDATA%/kimi-desktop/daimon-storage.json` 的 `shareDir`）
//! - **Kimi Code**:
//!   - `~/.kimi-code/sessions/`（支持环境变量 `KIMI_CODE_HOME` 覆盖）
//! - **Kimi CLI**:
//!   - `~/.kimi/sessions/`
//!
//! 目录结构：
//! - 会话根目录下存在若干工作区目录 `wd_*` 或直接为会话目录。
//! - 每个会话目录内包含 `state.json`（存放会话 id、标题、工作区路径）以及 `agents/*/wire.jsonl`（事件流）。
//! - 提取 `wire.jsonl` 中 `type == "usage.record"` 的事件，格式：
//!   `{"type":"usage.record","model":"...","usage":{"inputOther":10,"output":20,"inputCacheRead":30,"inputCacheCreation":0},"time":1789482715931}`
//!
//! 解析后的数据增量写入应用自有库 `pricing.db::session_request_logs`，`source = "Kimi"`。
//! 会话与标题写入 `sessions` 和 `session_titles` 表。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use serde_json::Value;

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::{metadata_modified_nanos, DshScanResult, ParsedRow};

/// Kimi 数据源标识（同时也用作 provider_id 与 session_request_logs.source）
pub const KIMI_SOURCE: &str = "Kimi";

/// 会话元数据（从 state.json 中解析）
#[derive(Debug, Clone, Default)]
pub struct KimiSessionMetadata {
    pub session_id: String,
    pub project_dir: String,
    pub title: String,
}

/// 清理会话标题中的 `<meta ... />` 标签与首尾空白
pub fn clean_kimi_title(raw_title: &str) -> String {
    let trimmed = raw_title.trim();
    if !trimmed.starts_with("<meta") {
        return trimmed.to_string();
    }
    if let Some(close_idx) = trimmed.find("/>") {
        let after = trimmed[close_idx + 2..].trim();
        if !after.is_empty() {
            return after.to_string();
        }
    }
    trimmed.to_string()
}

/// 从会话目录读取 state.json
pub fn read_kimi_state_metadata(session_dir: &Path) -> KimiSessionMetadata {
    let mut meta = KimiSessionMetadata::default();
    let dir_name = session_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    meta.session_id = dir_name.clone();

    let state_path = session_dir.join("state.json");
    if !state_path.is_file() {
        return meta;
    }

    let text = match std::fs::read_to_string(&state_path) {
        Ok(t) => t,
        Err(_) => return meta,
    };

    let v: Value = match serde_json::from_str(&text) {
        Ok(val) => val,
        Err(_) => return meta,
    };

    // session id
    if let Some(id_str) = v.get("id").and_then(|i| i.as_str()) {
        if !id_str.trim().is_empty() {
            meta.session_id = id_str.trim().to_string();
        }
    }

    // project / workDir
    let project = v
        .get("workDir")
        .and_then(|w| w.as_str())
        .or_else(|| v.get("cwd").and_then(|c| c.as_str()))
        .or_else(|| {
            v.get("custom")
                .and_then(|c| c.get("workspacePath"))
                .and_then(|p| p.as_str())
        })
        .unwrap_or("")
        .trim();
    meta.project_dir = project.to_string();

    // title
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .or_else(|| v.get("lastPrompt").and_then(|p| p.as_str()))
        .unwrap_or("");
    let cleaned = clean_kimi_title(title);
    meta.title = if cleaned.is_empty() {
        meta.session_id.clone()
    } else {
        cleaned
    };

    meta
}

/// 解析一行 wire.jsonl
///
/// 格式示例：
/// `{"type":"usage.record","model":"k28-agent-preview","usage":{"inputOther":7159,"output":371,"inputCacheRead":13056,"inputCacheCreation":0},"usageScope":"turn","time":1789482715931}`
pub fn parse_kimi_line(
    line: &str,
    session_id: &str,
    project_dir: &str,
    agent_name: &str,
    line_number: i64,
) -> Option<ParsedRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let event_type = v.get("type").and_then(|t| t.as_str())?;
    if event_type != "usage.record" {
        return None;
    }

    let usage = v.get("usage")?;
    let num = |key: &str| -> i64 {
        usage
            .get(key)
            .and_then(|x| x.as_i64())
            .unwrap_or(0)
    };

    let input_tokens = num("inputOther");
    let output_tokens = num("output");
    let cache_read = num("inputCacheRead");
    let cache_creation = num("inputCacheCreation");

    if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_creation == 0 {
        return None;
    }

    let model = v
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("kimi")
        .to_string();

    // time 毫秒转换为秒
    let time_raw = v.get("time").and_then(|t| t.as_i64()).unwrap_or(0);
    let created_at = if time_raw > 10_000_000_000 {
        time_raw / 1000
    } else {
        time_raw
    };

    // 格式化唯一且幂等的 request_id
    let request_id = format!("{}:{}:{}", session_id, agent_name, line_number);

    Some(ParsedRow {
        request_id,
        session_id: Some(session_id.to_string()),
        model,
        input_tokens,
        output_tokens,
        cache_read,
        cache_creation,
        created_at,
        project: project_dir.to_string(),
        latency: 0,
    })
}

/// 获取所有候选的 Kimi 会话根目录
pub fn get_all_kimi_session_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return roots,
    };

    // 1. Kimi Code
    if let Ok(env_home) = std::env::var("KIMI_CODE_HOME") {
        let env_trimmed = env_home.trim();
        if !env_trimmed.is_empty() {
            let p = PathBuf::from(env_trimmed);
            let sess = if p.file_name().and_then(|n| n.to_str()) == Some("sessions") {
                p
            } else {
                p.join("sessions")
            };
            if sess.is_dir() {
                roots.push(sess);
            }
        }
    }
    let default_kimi_code = home.join(".kimi-code").join("sessions");
    if default_kimi_code.is_dir() && !roots.contains(&default_kimi_code) {
        roots.push(default_kimi_code);
    }

    // 2. Kimi Work (kimi-desktop)
    #[cfg(target_os = "macos")]
    {
        let mac_work = home
            .join("Library")
            .join("Application Support")
            .join("kimi-desktop")
            .join("daimon-share")
            .join("daimon")
            .join("runtime")
            .join("kimi-code")
            .join("home")
            .join("sessions");
        if mac_work.is_dir() && !roots.contains(&mac_work) {
            roots.push(mac_work);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::data_dir() {
            // 查看 daimon-storage.json 是否配置了 shareDir
            let storage_json = appdata.join("kimi-desktop").join("daimon-storage.json");
            if storage_json.is_file() {
                if let Ok(content) = std::fs::read_to_string(&storage_json) {
                    if let Ok(json) = serde_json::from_str::<Value>(&content) {
                        if let Some(share_dir) = json.get("shareDir").and_then(|s| s.as_str()) {
                            let share_sessions = PathBuf::from(share_dir.trim())
                                .join("daimon")
                                .join("runtime")
                                .join("kimi-code")
                                .join("home")
                                .join("sessions");
                            if share_sessions.is_dir() && !roots.contains(&share_sessions) {
                                roots.push(share_sessions);
                            }
                        }
                    }
                }
            }

            let default_win_work = appdata
                .join("kimi-desktop")
                .join("daimon-share")
                .join("daimon")
                .join("runtime")
                .join("kimi-code")
                .join("home")
                .join("sessions");
            if default_win_work.is_dir() && !roots.contains(&default_win_work) {
                roots.push(default_win_work);
            }
        }
    }

    // 3. Kimi CLI (历史兼容)
    let kimi_cli = home.join(".kimi").join("sessions");
    if kimi_cli.is_dir() && !roots.contains(&kimi_cli) {
        roots.push(kimi_cli);
    }

    roots
}

/// 返回主要/推荐展示的 Kimi 目录路径（供 get_default_paths 使用）
pub fn primary_kimi_dir() -> Option<PathBuf> {
    let roots = get_all_kimi_session_roots();
    if let Some(first) = roots.first() {
        return Some(first.clone());
    }
    // 默认展示路径
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let mac_work = home
                .join("Library")
                .join("Application Support")
                .join("kimi-desktop")
                .join("daimon-share")
                .join("daimon")
                .join("runtime")
                .join("kimi-code")
                .join("home")
                .join("sessions");
            return Some(mac_work);
        }
    }
    dirs::home_dir().map(|h| h.join(".kimi-code").join("sessions"))
}

/// 检查是否存在 Kimi 数据源
pub fn kimi_source_available() -> bool {
    !get_all_kimi_session_roots().is_empty()
}

/// 结构：一个待扫描的 wire.jsonl 文件与其所属会话元数据
#[derive(Debug, Clone)]
struct KimiTargetFile {
    pub wire_path: PathBuf,
    pub session_id: String,
    pub project_dir: String,
    pub title: String,
    pub agent_name: String,
}

/// 扫描单个根目录下所有会话中的 wire.jsonl 文件
fn collect_wire_files_in_root(root: &Path, out: &mut Vec<KimiTargetFile>) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // 检查 path 本身是否是一个 session 目录（包含 state.json 或 agents 子目录）
        if path.join("state.json").is_file() || path.join("agents").is_dir() {
            collect_wires_from_session_dir(&path, out);
        } else {
            // 否则可能是工作区目录（如 wd_*），深入一层扫描其中的会话目录
            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    let sub_path = sub_entry.path();
                    if sub_path.is_dir()
                        && (sub_path.join("state.json").is_file() || sub_path.join("agents").is_dir())
                    {
                        collect_wires_from_session_dir(&sub_path, out);
                    }
                }
            }
        }
    }
}

fn collect_wires_from_session_dir(session_dir: &Path, out: &mut Vec<KimiTargetFile>) {
    let meta = read_kimi_state_metadata(session_dir);
    let agents_dir = session_dir.join("agents");
    if !agents_dir.is_dir() {
        return;
    }

    let agent_entries = match std::fs::read_dir(&agents_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for agent_entry in agent_entries.flatten() {
        let agent_path = agent_entry.path();
        if !agent_path.is_dir() {
            continue;
        }
        let agent_name = agent_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main")
            .to_string();
        let wire_file = agent_path.join("wire.jsonl");
        if wire_file.is_file() {
            out.push(KimiTargetFile {
                wire_path: wire_file,
                session_id: meta.session_id.clone(),
                project_dir: meta.project_dir.clone(),
                title: meta.title.clone(),
                agent_name,
            });
        }
    }
}

/// 返回所有 Kimi session 文件中最新的修改元数据（供 refresh 检测）
pub fn latest_session_file_mtime() -> Option<std::fs::Metadata> {
    let roots = get_all_kimi_session_roots();
    let mut targets = Vec::new();
    for root in &roots {
        collect_wire_files_in_root(root, &mut targets);
    }

    targets
        .into_iter()
        .filter_map(|t| std::fs::metadata(&t.wire_path).ok())
        .max_by_key(|m| metadata_modified_nanos(m))
}

/// 增量扫描单个 wire.jsonl
fn scan_kimi_wire_incremental(
    app_db: &AppDbService,
    target: &KimiTargetFile,
) -> Result<(u32, u32), String> {
    let file_path_str = target.wire_path.to_string_lossy().to_string();
    let metadata = std::fs::metadata(&target.wire_path)
        .map_err(|e| format!("读取文件元数据失败: {}", e))?;
    let file_modified = metadata_modified_nanos(&metadata);

    let (last_modified, last_offset) = app_db
        .get_session_log_sync_state(KIMI_SOURCE, &file_path_str)
        .unwrap_or((0, 0));

    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let text = std::fs::read_to_string(&target.wire_path)
        .map_err(|e| format!("读取 wire.jsonl 失败: {}", e))?;

    let conn = app_db.conn();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("开启事务失败: {}", e))?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut line_offset = 0i64;

    for line in text.lines() {
        line_offset += 1;
        if line_offset <= last_offset {
            continue;
        }

        if let Some(msg) = parse_kimi_line(
            line,
            &target.session_id,
            &target.project_dir,
            &target.agent_name,
            line_offset,
        ) {
            let request_id = format!("{}:{}", KIMI_SOURCE, msg.request_id);
            match AppDbService::insert_session_log_on_conn(
                &tx,
                KIMI_SOURCE,
                &request_id,
                msg.session_id.as_deref().unwrap_or(""),
                &msg.model,
                KIMI_SOURCE,
                msg.input_tokens,
                msg.output_tokens,
                msg.cache_read,
                msg.cache_creation,
                msg.created_at,
                msg.latency,
                0,
            ) {
                Ok(true) => imported += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    log::warn!("[KIMI-SYNC] 插入失败 ({}): {}", msg.request_id, e);
                    skipped += 1;
                }
            }
        }
    }

    // 会话项目归属与标题入库
    if !target.session_id.is_empty() {
        let _ = tx.execute(
            "INSERT INTO sessions (session_id, project_dir, title, source)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE SET
               project_dir = CASE WHEN sessions.project_dir = '' THEN excluded.project_dir ELSE sessions.project_dir END,
               title = CASE WHEN sessions.title = '' THEN excluded.title ELSE sessions.title END",
            rusqlite::params![
                target.session_id,
                target.project_dir,
                target.title,
                KIMI_SOURCE,
            ],
        );

        let _ = tx.execute(
            "INSERT OR REPLACE INTO session_titles (session_id, title, source, created_at)
             VALUES (?1, ?2, ?3, strftime('%s','now'))",
            rusqlite::params![target.session_id, target.title, KIMI_SOURCE],
        );
    }

    AppDbService::update_session_log_sync_on_conn(
        &tx,
        KIMI_SOURCE,
        &file_path_str,
        file_modified,
        line_offset,
    )?;

    tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;

    Ok((imported, skipped))
}

/// 执行 Kimi 会话全量/增量扫描
pub fn scan_kimi(app_db: &AppDbService) -> Result<DshScanResult, String> {
    let roots = get_all_kimi_session_roots();
    scan_kimi_roots(app_db, &roots)
}

/// 指定根目录列表执行扫描（用于测试与定制）
pub fn scan_kimi_roots(app_db: &AppDbService, roots: &[PathBuf]) -> Result<DshScanResult, String> {
    let mut targets = Vec::new();
    for root in roots {
        if root.is_dir() {
            collect_wire_files_in_root(root, &mut targets);
        }
    }

    // 按路径排序保证扫描稳定性
    targets.sort_by(|a, b| a.wire_path.cmp(&b.wire_path));

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;

    let mut seen_sessions = HashSet::new();

    for target in &targets {
        seen_sessions.insert(target.session_id.clone());
        match scan_kimi_wire_incremental(app_db, target) {
            Ok((imp, skp)) => {
                imported += imp;
                skipped += skp;
            }
            Err(e) => {
                log::warn!("[KIMI-SCAN] 扫描文件失败 {:?}: {}", target.wire_path, e);
                errors += 1;
            }
        }
    }

    let total = app_db.get_session_log_count(KIMI_SOURCE).unwrap_or(0);

    Ok(DshScanResult {
        files_scanned: targets.len() as u32,
        imported,
        skipped,
        errors,
        total_records: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_kimi_title() {
        assert_eq!(
            clean_kimi_title("<meta awareness=\"low\" timestamp=\"2026-09-15 22:31\" /> 你支持哪些工具"),
            "你支持哪些工具"
        );
        assert_eq!(
            clean_kimi_title("普通标题没有meta标签"),
            "普通标题没有meta标签"
        );
        assert_eq!(
            clean_kimi_title("<meta awareness=\"low\" />"),
            "<meta awareness=\"low\" />"
        );
    }

    #[test]
    fn test_parse_kimi_line() {
        let line = r#"{"type":"usage.record","model":"k28-agent-preview","usage":{"inputOther":7159,"output":371,"inputCacheRead":13056,"inputCacheCreation":0},"usageScope":"turn","time":1789482715931}"#;
        let parsed = parse_kimi_line(line, "sess-1", "/my/proj", "main", 1).unwrap();
        assert_eq!(parsed.session_id.unwrap(), "sess-1");
        assert_eq!(parsed.model, "k28-agent-preview");
        assert_eq!(parsed.input_tokens, 7159);
        assert_eq!(parsed.output_tokens, 371);
        assert_eq!(parsed.cache_read, 13056);
        assert_eq!(parsed.cache_creation, 0);
        assert_eq!(parsed.created_at, 1789482715);
        assert_eq!(parsed.project, "/my/proj");
        assert_eq!(parsed.request_id, "sess-1:main:1");

        // 非 usage.record 行返回 None
        let other = r#"{"type":"turn.prompt","input":[{"type":"text","text":"hi"}]}"#;
        assert!(parse_kimi_line(other, "sess-1", "/my/proj", "main", 2).is_none());

        // 全 0 token 行跳过
        let zero = r#"{"type":"usage.record","model":"kimi","usage":{"inputOther":0,"output":0,"inputCacheRead":0,"inputCacheCreation":0},"time":1789482715931}"#;
        assert!(parse_kimi_line(zero, "sess-1", "/my/proj", "main", 3).is_none());
    }

    #[test]
    fn test_scan_kimi_incremental() {
        // 1. 创建临时数据库
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("pricing.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session_request_logs (
                request_id TEXT PRIMARY KEY, source TEXT, session_id TEXT, model TEXT,
                provider_id TEXT, input_tokens INTEGER, output_tokens INTEGER,
                cache_read INTEGER, cache_creation INTEGER,
                created_at INTEGER NOT NULL, latency INTEGER NOT NULL DEFAULT 0,
                first_token_latency INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE session_log_sync (
                file_path TEXT NOT NULL, source TEXT NOT NULL,
                last_modified INTEGER NOT NULL, last_line_offset INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (file_path, source)
            );
            CREATE TABLE sessions (
                session_id TEXT PRIMARY KEY, project_dir TEXT, title TEXT, source TEXT
            );
            CREATE TABLE session_titles (
                session_id TEXT PRIMARY KEY, title TEXT, source TEXT, created_at INTEGER
            );",
        ).unwrap();

        // 2. 模拟 Kimi 目录结构
        let kimi_root = temp_dir.path().join("kimi_root");
        let session_dir = kimi_root.join("wd_test_123").join("conv-abc");
        let agent_dir = session_dir.join("agents").join("main");
        std::fs::create_dir_all(&agent_dir).unwrap();

        let state_json = r#"{
            "id": "conv-abc",
            "workDir": "/test/workspace",
            "title": "<meta awareness=\"low\" /> Kimi会话测试"
        }"#;
        std::fs::write(session_dir.join("state.json"), state_json).unwrap();

        let wire_file = agent_dir.join("wire.jsonl");
        let line1 = r#"{"type":"usage.record","model":"k28-agent-preview","usage":{"inputOther":100,"output":50,"inputCacheRead":20,"inputCacheCreation":0},"time":1789482715000}"#;
        std::fs::write(&wire_file, format!("{}\n", line1)).unwrap();

        // 验证 metadata 读取
        let meta = read_kimi_state_metadata(&session_dir);
        assert_eq!(meta.session_id, "conv-abc");
        assert_eq!(meta.project_dir, "/test/workspace");
        assert_eq!(meta.title, "Kimi会话测试");

        let files = {
            let mut out = Vec::new();
            collect_wire_files_in_root(&kimi_root, &mut out);
            out
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].session_id, "conv-abc");
    }

    #[test]
    fn test_scan_real_machine_kimi() {
        if !kimi_source_available() {
            println!("[TEST] 本机未检测到 Kimi 目录，跳过真实数据扫描测试");
            return;
        }
        let roots = get_all_kimi_session_roots();
        println!("[TEST] 本机发现 Kimi 根目录: {:?}", roots);

        // 使用内存数据库执行真实数据扫描
        let app_db = AppDbService::new_in_memory().unwrap();

        let r1 = scan_kimi(&app_db).unwrap();
        println!(
            "[TEST] 首次扫描结果: files={}, imported={}, skipped={}, total={}",
            r1.files_scanned, r1.imported, r1.skipped, r1.total_records
        );
        assert!(r1.files_scanned > 0, "应扫描到至少一个会话文件");
        assert!(r1.imported > 0, "应成功导入用量记录");

        // 再次扫描，应命中增量游标全部跳过
        let r2 = scan_kimi(&app_db).unwrap();
        println!(
            "[TEST] 二次扫描结果: files={}, imported={}, skipped={}, total={}",
            r2.files_scanned, r2.imported, r2.skipped, r2.total_records
        );
        assert_eq!(r2.imported, 0, "二次扫描不应导入重复记录");
    }
}
