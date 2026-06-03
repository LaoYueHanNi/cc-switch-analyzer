use std::collections::HashMap;
use std::collections::HashSet;
use tauri::State;

use crate::AppState;
use crate::models::*;
use crate::services::multi_terminal::{
    agent_kind_from_source, build_pane_command, build_wt_args, PaneSpec,
};

/// 列出所有任务（含聚合统计：会话数 / Token / 费用）
#[tauri::command]
pub fn list_tasks(state: State<AppState>) -> Result<Vec<TaskWithStats>, String> {
    let tasks = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        app_db.list_tasks()?
    };

    if tasks.is_empty() {
        return Ok(vec![]);
    }

    // 聚合统计:遍历 task_sessions,按 session_id+source 去重
    // 费用/Token 从外部数据源按 session_id 实时聚合
    let task_sessions_map = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let mut m: HashMap<i64, Vec<TaskSession>> = HashMap::new();
        for t in &tasks {
            m.insert(t.id, app_db.list_task_sessions(t.id)?);
        }
        m
    };

    // 收集所有需要查询 token/cost 的 sessionid(去重)
    let mut all_session_ids: Vec<String> = Vec::new();
    for sessions in task_sessions_map.values() {
        for s in sessions {
            if !all_session_ids.contains(&s.session_id) {
                all_session_ids.push(s.session_id.clone());
            }
        }
    }

    // 从外部数据源聚合(若数据源未加载则返回空 map,优雅降级)
    let (token_map, cost_map) = aggregate_by_session_ids(&state, &all_session_ids);

    // 拼装结果
    let mut out = Vec::with_capacity(tasks.len());
    for t in tasks {
        let sessions = task_sessions_map.get(&t.id).cloned().unwrap_or_default();
        let mut total_tokens: i64 = 0;
        let mut total_cost: f64 = 0.0;
        for s in &sessions {
            total_tokens += token_map.get(&s.session_id).copied().unwrap_or(0);
            total_cost += cost_map.get(&s.session_id).copied().unwrap_or(0.0);
        }
        out.push(TaskWithStats {
            task: t,
            session_count: sessions.len() as i64,
            total_tokens,
            total_cost,
            sessions,
        });
    }
    Ok(out)
}

/// 获取任务详情:任务元信息 + 关联的 task_sessions
#[tauri::command]
pub fn get_task_detail(
    task_id: i64,
    state: State<AppState>,
) -> Result<TaskDetail, String> {
    let (task, sessions) = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let t = app_db
            .get_task(task_id)?
            .ok_or_else(|| format!("任务 {} 不存在", task_id))?;
        let s = app_db.list_task_sessions(task_id)?;
        (t, s)
    };

    // 聚合 token/cost
    let session_ids: Vec<String> = sessions.iter().map(|s| s.session_id.clone()).collect();
    let (token_map, cost_map) = aggregate_by_session_ids(&state, &session_ids);
    let mut total_tokens: i64 = 0;
    let mut total_cost: f64 = 0.0;
    for sid in &session_ids {
        total_tokens += token_map.get(sid).copied().unwrap_or(0);
        total_cost += cost_map.get(sid).copied().unwrap_or(0.0);
    }

    Ok(TaskDetail {
        task,
        session_count: sessions.len() as i64,
        total_tokens,
        total_cost,
        sessions,
    })
}

/// 在任务内获取单个会话的完整详情(走标准 pipeline)
#[tauri::command]
pub fn get_task_session_detail(
    task_id: i64,
    session_id: String,
    state: State<AppState>,
) -> Result<crate::models::ProjectSessionDetail, String> {
    // 校验该 session 属于该 task
    {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        let sessions = app_db.list_task_sessions(task_id)?;
        if !sessions.iter().any(|s| s.session_id == session_id) {
            return Err(format!(
                "会话 {} 不属于任务 {}",
                session_id, task_id
            ));
        }
    }
    // 复用既有命令
    crate::commands::query::query_project_session_details(
        FilterParams {
            from_epoch: None,
            to_epoch: None,
            tz_offset: None,
            provider_id: None,
            model_id: None,
        },
        vec![session_id],
        state,
    )
    .and_then(|mut v| {
        v.pop()
            .ok_or_else(|| "未找到会话详情".to_string())
    })
}

#[tauri::command]
pub fn create_task(
    title: String,
    description: String,
    status: String,
    state: State<AppState>,
) -> Result<i64, String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.create_task(&title, &description, &status)
}

#[tauri::command]
pub fn update_task(
    task_id: i64,
    title: String,
    description: String,
    status: String,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.update_task(task_id, &title, &description, &status)
}

#[tauri::command]
pub fn delete_task(task_id: i64, state: State<AppState>) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.delete_task(task_id)
}

#[tauri::command]
pub fn add_sessions_to_task(
    task_id: i64,
    sessions: Vec<TaskSessionInput>,
    state: State<AppState>,
) -> Result<(), String> {
    let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
    app_db.add_task_sessions(task_id, &sessions)
}

/// 任务内的「开新会话」入口:根据 agent_source + provider_id + 目录
/// 调用 session_manager 中已有的开终端函数,行为与会话 tab 一致。
#[tauri::command]
pub fn open_task_agent(
    agent_source: String,
    project_dir: String,
    provider_id: Option<String>,
    db_path: Option<String>,
) -> Result<(), String> {
    match agent_source.as_str() {
        "claude" => {
            if let (Some(pid), Some(path)) = (provider_id.as_ref(), db_path.as_ref()) {
                crate::commands::session_manager::open_claude_terminal_with_provider(
                    project_dir, pid.clone(), path.clone(),
                )
            } else {
                crate::commands::session_manager::open_claude_terminal(project_dir)
            }
        }
        "opencode" => {
            crate::commands::session_manager::open_opencode_terminal(project_dir)
        }
        "codex" => {
            crate::commands::session_manager::open_codex_terminal(project_dir)
        }
        other => Err(format!("不支持的 agent 类型: {}", other)),
    }
}

/// 一键恢复任务下所有会话:按 4-pane/tab 布局规则,在 Windows Terminal
/// 里恢复所有已绑定的 session。
///
/// 返回 `(spawned, total)`:
/// - `spawned` 成功 spawn 的 pane 数
/// - `total` 任务下原始 session 数(去重前;若>0 但 spawned=0 表示全部被去重或过滤)
#[tauri::command]
pub fn open_task_sessions(
    task_id: i64,
    state: State<AppState>,
) -> Result<OpenTaskSessionsResult, String> {
    // 1. 拉 task_sessions
    let raw_sessions = {
        let app_db = state.app_db.lock().map_err(|e| e.to_string())?;
        app_db.list_task_sessions(task_id)?
    };
    let total = raw_sessions.len();

    // 2. 去重 + 过滤 + 转 PaneSpec
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut specs: Vec<PaneSpec> = Vec::with_capacity(raw_sessions.len());
    for s in raw_sessions {
        if s.session_id.is_empty() {
            continue;
        }
        let key = (s.session_id.clone(), s.source.clone());
        if !seen.insert(key) {
            continue;
        }
        let agent = match agent_kind_from_source(&s.source) {
            Some(a) => a,
            // 未知 source 跳过(不报错,避免脏数据阻塞整个 task)
            None => continue,
        };
        let project_dir = if s.project_dir.is_empty() {
            None
        } else {
            Some(s.project_dir)
        };
        specs.push(PaneSpec {
            agent,
            session_id: Some(s.session_id),
            project_dir,
        });
    }

    if specs.is_empty() {
        return Ok(OpenTaskSessionsResult { spawned: 0, total });
    }

    // 3. 拼参数 + spawn(仅 Windows)
    #[cfg(target_os = "windows")]
    {
        let args = build_wt_args(&specs, build_pane_command);
        std::process::Command::new("wt")
            .args(&args)
            .spawn()
            .map_err(|e| format!("启动终端失败: {}", e))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        return Err("当前仅支持 Windows".to_string());
    }

    Ok(OpenTaskSessionsResult {
        spawned: specs.len(),
        total,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTaskSessionsResult {
    pub spawned: usize,
    pub total: usize,
}

// ========== 内部辅助 ==========

/// 按 session_id 列表聚合 token/cost(无数据源时返回空 map,优雅降级)
fn aggregate_by_session_ids(
    state: &State<AppState>,
    session_ids: &[String],
) -> (HashMap<String, i64>, HashMap<String, f64>) {
    if session_ids.is_empty() {
        return (HashMap::new(), HashMap::new());
    }
    let sources = match state.data_sources.read() {
        Ok(s) => s,
        Err(_) => return (HashMap::new(), HashMap::new()),
    };
    if sources.is_empty() {
        return (HashMap::new(), HashMap::new());
    }
    let params = FilterParams {
        from_epoch: None,
        to_epoch: None,
        tz_offset: None,
        provider_id: None,
        model_id: None,
    };
    // 从每个数据源拉该批 sessionid 的 request_tokens,合并去重
    let set: HashSet<String> = session_ids.iter().cloned().collect();
    let mut all_req: Vec<SessionRequestToken> = Vec::new();
    for entry in sources.iter() {
        if let Ok(items) = entry.source.get_session_request_tokens_for_ids(&params, session_ids) {
            all_req.extend(items);
        }
    }
    let all_req = crate::services::dedup::dedup_request_tokens(all_req);
    // 用 pipeline 的 aggregate_session_breakdown 算 token
    let mut token_map: HashMap<String, i64> = HashMap::new();
    let pricing = state.pricing_engine.read().ok();
    for req in &all_req {
        if !set.contains(&req.session_id) {
            continue;
        }
        let total = req.input_tokens
            + req.output_tokens
            + req.cache_read
            + req.cache_creation;
        token_map
            .entry(req.session_id.clone())
            .and_modify(|v| *v += total)
            .or_insert(total);
    }
    // 算 cost
    let mut cost_map: HashMap<String, f64> = HashMap::new();
    if let Some(pricing) = pricing {
        let cost_lookup = crate::services::precompute::compute_session_costs(&all_req, &pricing);
        for (sid, cost) in cost_lookup {
            if set.contains(&sid) {
                cost_map.insert(sid, cost);
            }
        }
    }
    (token_map, cost_map)
}
