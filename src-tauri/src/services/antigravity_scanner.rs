//! Antigravity 用量扫描器(本地 language server RPC)
//!
//! Antigravity 与其他本地源的根本差异:token 用量**不落盘**。
//! `~/.gemini/antigravity/brain/<cascadeId>/` 下只有对话转录文本
//! (transcript.jsonl)与 artifact,没有任何 token 字段;用量只能向
//! **正在运行的** language server 用 Connect RPC 问出来:
//!
//! - `GetCascadeTrajectoryGeneratorMetadata` → 每轮生成的 usage(token 明细)
//! - `GetCascadeTrajectory` → 每步 createdAt,按 responseId / messageId 回填时间戳
//!
//! 因此扫描流程为「文件系统枚举会话 id → RPC 取用量 → 写入 session_request_logs」。
//! 落库后与 ZCode / DSH / MiniMax 完全同构:同一张表、同一套增量游标、只追加不
//! 随源删除,查询侧(antigravity_db)读的就是本地库,IDE 关掉后历史用量照常可看。
//! IDE 未运行时取不到**新**数据,这一点由 `probe_endpoint` 的错误信息告知前端。
//!
//! 去重与增量(对齐 zcode_scanner,`session_log_sync` 以会话目录为 key):
//! - `request_id` = `Antigravity:<responseId>`,主键天然去重(重复扫描不会双计)
//! - `last_modified` 存会话目录 mtime 纳秒,未变化的会话直接跳过,省掉两次 RPC
//! - `last_line_offset` 存上轮取到的用量条数,用于判断游标能否推进(见
//!   [`should_advance_cursor`])——这是与 ZCode 的关键差异:源库的数据"写完才可见",
//!   而 RPC 可能滞后于文件 mtime

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::services::app_db::AppDbService;
use crate::services::dsh_scanner::DshScanResult;

/// session_request_logs.source / provider_id 的 canonical 值(= DbType::label)
pub const ANTIGRAVITY_SOURCE: &str = "Antigravity";

/// Connect RPC 服务前缀
const LS_SERVICE: &str = "exa.language_server_pb.LanguageServerService";

/// `~/.gemini` 下的 Antigravity 数据根:2.0 / IDE / 备份
const DATA_ROOTS: [&str; 3] = ["antigravity", "antigravity-ide", "antigravity-backup"];

const RPC_TIMEOUT: Duration = Duration::from_secs(20);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// 会话静置多久后视为"已稳定":此前即使没取到新用量也不推进游标,留给
/// 正在流式生成的那一轮补齐的机会。取值需明显长于单次响应耗时。
const RESCAN_GRACE_SECS: i64 = 600;
/// 单次 RPC 响应上限(GetCascadeTrajectory 含全文,长会话可达数 MB)
const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;

// ========== 不可达退避 ==========
//
// IDE 未运行是常态:pending 会话存在时每轮采集都要做进程枚举 + 端口探测
// 并以失败告终。连续失败按 5s→10s→20s→40s→60s(封顶)退避,期间后台采集
// 直接跳过;成功立即重置。手动扫描命令走 scan_antigravity,不受退避限制。

struct AntigravityBackoff {
    failures: u32,
    next: Instant,
}

static BACKOFF: Mutex<Option<AntigravityBackoff>> = Mutex::new(None);

/// 第 n 次连续失败后的等待时长(n 从 1 起):5s, 10s, 20s, 40s, 之后封顶 60s
fn backoff_delay_secs(failures: u32) -> u64 {
    (5u64 << (failures - 1).min(4)).min(60)
}

fn backoff_allows_try() -> bool {
    BACKOFF
        .lock()
        .map(|s| s.as_ref().map_or(true, |b| Instant::now() >= b.next))
        .unwrap_or(true)
}

fn backoff_record_failure() {
    if let Ok(mut guard) = BACKOFF.lock() {
        let failures = guard.as_ref().map_or(0, |b| b.failures) + 1;
        *guard = Some(AntigravityBackoff {
            failures,
            next: Instant::now() + Duration::from_secs(backoff_delay_secs(failures)),
        });
    }
}

fn backoff_reset() {
    if let Ok(mut guard) = BACKOFF.lock() {
        *guard = None;
    }
}

/// 是否为「language server 不可达」类失败(其它错误如应用库故障不退避,照常重试)
fn is_endpoint_unavailable(err: &str) -> bool {
    err.starts_with("未发现运行中的 Antigravity")
        || err.starts_with("Antigravity language server 已在运行但 RPC 不可达")
}


// ========== language server 发现 ==========

/// 单个待探活端点候选
#[derive(Debug, Clone, PartialEq, Eq)]
struct EndpointCandidate {
    pid: u32,
    scheme: &'static str,
    port: u16,
    csrf_token: String,
    source: &'static str,
}

/// 从 `daemon/*.json` 解析出的连接元数据。
///
/// Antigravity 2.34+ 不再在 `language_server` 命令行暴露 `--csrf_token`,
/// 改为把运行时的 `pid` / `httpPort` / `httpsPort` / `csrfToken` 写入
/// `~/.gemini/antigravity*/daemon/hub_<httpPort>.json`。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DaemonDescriptor {
    pid: u32,
    http_port: Option<u16>,
    https_port: Option<u16>,
    csrf_token: String,
}

/// 一个候选 language server 进程(旧版命令行发现路径)
#[derive(Debug, Clone)]
struct LsProcess {
    pid: u32,
    csrf_token: String,
    ports: Vec<u16>,
}

/// 已探活的 RPC 端点
#[derive(Debug, Clone)]
pub struct LsEndpoint {
    scheme: &'static str,
    port: u16,
    csrf_token: String,
}

impl LsEndpoint {
    fn url(&self, method: &str) -> String {
        format!(
            "{}://127.0.0.1:{}/{}/{}",
            self.scheme, self.port, LS_SERVICE, method
        )
    }
}

/// 判断进程 PID 是否仍存活(零子进程、微秒级,用于过滤 `daemon/` 下残留的旧文件)。
#[cfg(target_os = "windows")]
fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    const ERROR_ACCESS_DENIED: i32 = 5;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn GetExitCodeProcess(handle: *mut std::ffi::c_void, exit_code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // 无权限打开目标进程句柄仍说明进程存在
            return std::io::Error::last_os_error().raw_os_error() == Some(ERROR_ACCESS_DENIED);
        }
        let mut exit_code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        ok != 0 && exit_code == STILL_ACTIVE
    }
}

#[cfg(not(target_os = "windows"))]
fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    const EPERM: i32 = 1;
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        if kill(pid as i32, 0) == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(EPERM)
    }
}

fn json_port(value: Option<&Value>) -> Option<u16> {
    let raw = match value {
        Some(Value::Number(n)) => n.as_u64()?,
        Some(Value::String(s)) => s.trim().parse::<u64>().ok()?,
        _ => return None,
    };
    if raw == 0 || raw > u16::MAX as u64 {
        None
    } else {
        Some(raw as u16)
    }
}

/// 解析 `daemon/*.json` 的连接元数据。
fn parse_daemon_descriptor(content: &str) -> Option<DaemonDescriptor> {
    let v: Value = serde_json::from_str(content).ok()?;
    let csrf_token = json_str(v.get("csrfToken"));
    if csrf_token.is_empty() {
        return None;
    }
    let http_port = json_port(v.get("httpPort"));
    let https_port = json_port(v.get("httpsPort"));
    if http_port.is_none() && https_port.is_none() {
        return None;
    }
    let pid = match v.get("pid") {
        Some(Value::Number(n)) => n.as_u64().and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse::<u32>().unwrap_or(0),
        _ => 0,
    };
    Some(DaemonDescriptor {
        pid,
        http_port,
        https_port,
        csrf_token,
    })
}

/// 从给定数据根的 `daemon/*.json` 收集候选端点:按修改时间降序,过滤已退出的 PID,
/// 每个存活实例优先尝试明文 `httpPort` 再尝试 `httpsPort`。
fn collect_daemon_candidates_in<F>(roots: &[PathBuf], pid_alive: F) -> Vec<EndpointCandidate>
where
    F: Fn(u32) -> bool,
{
    let mut entries_with_mtime: Vec<(i64, String, DaemonDescriptor)> = Vec::new();
    for root in roots {
        let daemon_dir = root.join("daemon");
        let Ok(read_dir) = std::fs::read_dir(&daemon_dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(desc) = parse_daemon_descriptor(&content) else {
                continue;
            };
            if desc.pid > 0 && !pid_alive(desc.pid) {
                continue;
            }
            let mtime = entry
                .metadata()
                .map(|m| metadata_modified_nanos(&m))
                .unwrap_or(0);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            entries_with_mtime.push((mtime, name, desc));
        }
    }

    // 最新写入的描述文件排在最前,命中当前实例时首跳即通
    entries_with_mtime.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

    let mut out = Vec::new();
    for (_, _, desc) in entries_with_mtime {
        if let Some(port) = desc.http_port {
            out.push(EndpointCandidate {
                pid: desc.pid,
                scheme: "http",
                port,
                csrf_token: desc.csrf_token.clone(),
                source: "daemon",
            });
        }
        if let Some(port) = desc.https_port {
            out.push(EndpointCandidate {
                pid: desc.pid,
                scheme: "https",
                port,
                csrf_token: desc.csrf_token.clone(),
                source: "daemon",
            });
        }
    }
    out
}

fn detect_daemon_candidates() -> Vec<EndpointCandidate> {
    collect_daemon_candidates_in(&antigravity_data_roots(), is_pid_alive)
}

/// 执行外部命令取 stdout。Windows 下隐藏控制台窗口,避免扫描时闪黑框。
fn run_capture(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 从命令行抠出 `--flag value` / `--flag=value` 的值
fn extract_flag(command_line: &str, flag: &str) -> Option<String> {
    let idx = command_line.find(flag)?;
    let rest = &command_line[idx + flag.len()..];
    let rest = rest.trim_start_matches(['=', ' ', '\t']);
    let value: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// 判断命令行是否为 Antigravity 的 language server。
///
/// 同时要求「是 language server」与「路径/参数指向 antigravity」,
/// 避免误命中其他 Codeium 系产品(Windsurf 等)的同名进程。
fn is_antigravity_ls(command_line: &str) -> bool {
    let lower = command_line.to_lowercase();
    let is_ls = lower.contains("language_server") || lower.contains("language-server");
    if !is_ls {
        return false;
    }
    lower.contains("antigravity")
}

/// 解析一行 "pid\tcommandline"
fn parse_process_line(line: &str) -> Option<(u32, String)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (pid_str, command) = line.split_once('\t').or_else(|| line.split_once(' '))?;
    let pid: u32 = pid_str.trim().parse().ok()?;
    let command = command.trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some((pid, command))
}

#[cfg(target_os = "windows")]
fn detect_process_lines() -> Vec<String> {
    // Name 过滤放宽、由 is_antigravity_ls 精筛;CommandLine 才带 --csrf_token
    let script = "Get-CimInstance Win32_Process | \
         Where-Object { $_.Name -like 'language_server*' -or $_.Name -like 'language-server*' } | \
         ForEach-Object { \"$($_.ProcessId)`t$($_.CommandLine)\" }";
    run_capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
    .map(|s| s.lines().map(|l| l.to_string()).collect())
    .unwrap_or_default()
}

#[cfg(not(target_os = "windows"))]
fn detect_process_lines() -> Vec<String> {
    run_capture("ps", &["-ax", "-o", "pid=,command="])
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

#[cfg(target_os = "windows")]
fn listening_ports(pid: u32) -> Vec<u16> {
    let script = format!(
        "Get-NetTCPConnection -OwningProcess {} -State Listen -ErrorAction SilentlyContinue | \
         Select-Object -ExpandProperty LocalPort",
        pid
    );
    let mut ports: Vec<u16> = run_capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
    .map(|s| s.lines().filter_map(|l| l.trim().parse().ok()).collect())
    .unwrap_or_default();
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(not(target_os = "windows"))]
fn listening_ports(pid: u32) -> Vec<u16> {
    let pid_str = pid.to_string();
    let out = run_capture(
        "lsof",
        &["-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", &pid_str],
    )
    .unwrap_or_default();
    let mut ports: Vec<u16> = Vec::new();
    for line in out.lines() {
        // 形如 "... TCP 127.0.0.1:63185 (LISTEN)"
        if let Some(pos) = line.rfind("(LISTEN)") {
            let head = &line[..pos];
            if let Some(colon) = head.rfind(':') {
                let num: String = head[colon + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(p) = num.parse::<u16>() {
                    ports.push(p);
                }
            }
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// 枚举本机所有 Antigravity language server 候选
fn detect_processes() -> Vec<LsProcess> {
    let mut out = Vec::new();
    for line in detect_process_lines() {
        let Some((pid, command)) = parse_process_line(&line) else {
            continue;
        };
        if !is_antigravity_ls(&command) {
            continue;
        }
        // IDE / 桌面端的 language server 用 --csrf_token 认证本地请求;
        // 缺 token 的进程直接跳过(后续 RPC 必然 401)
        let Some(csrf_token) = extract_flag(&command, "--csrf_token") else {
            continue;
        };
        let ports = listening_ports(pid);
        if ports.is_empty() {
            continue;
        }
        out.push(LsProcess {
            pid,
            csrf_token,
            ports,
        });
    }
    out
}

fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .build()
        .into()
}

/// 发一次 Connect RPC。返回 Ok 表示 HTTP 200 且响应体为合法 JSON。
fn rpc_call(
    agent: &ureq::Agent,
    endpoint: &LsEndpoint,
    method: &str,
    body: &Value,
) -> Result<Value, String> {
    let payload = serde_json::to_string(body).map_err(|e| format!("构造 RPC 请求失败: {}", e))?;
    let mut response = agent
        .post(endpoint.url(method))
        .header("content-type", "application/json")
        // Connect 协议标识 + Codeium 系 language server 的本地 CSRF 校验
        .header("connect-protocol-version", "1")
        .header("x-codeium-csrf-token", &endpoint.csrf_token)
        .send(&payload)
        .map_err(|e| format!("{} 请求失败: {}", method, e))?;
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|e| format!("读取 {} 响应失败: {}", method, e))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 {} 响应失败: {}", method, e))
}

/// 将旧版命令行发现的进程展开为候选端点(每个端口先试明文 HTTP 再试 HTTPS)
fn detect_process_candidates() -> Vec<EndpointCandidate> {
    let mut out = Vec::new();
    for proc in detect_processes() {
        for port in proc.ports {
            for scheme in ["http", "https"] {
                out.push(EndpointCandidate {
                    pid: proc.pid,
                    scheme,
                    port,
                    csrf_token: proc.csrf_token.clone(),
                    source: "cmdline",
                });
            }
        }
    }
    out
}

fn try_probe_candidates(
    agent: &ureq::Agent,
    candidates: &[EndpointCandidate],
    seen: &mut std::collections::HashSet<(&'static str, u16, String)>,
    last_err: &mut String,
) -> Option<LsEndpoint> {
    let heartbeat = serde_json::json!({ "uuid": "00000000-0000-0000-0000-000000000000" });
    for c in candidates {
        if !seen.insert((c.scheme, c.port, c.csrf_token.clone())) {
            continue;
        }
        let endpoint = LsEndpoint {
            scheme: c.scheme,
            port: c.port,
            csrf_token: c.csrf_token.clone(),
        };
        match rpc_call(agent, &endpoint, "Heartbeat", &heartbeat) {
            Ok(_) => {
                log::info!(
                    "[ANTIGRAVITY] 端点可用: {}://127.0.0.1:{} (pid={}, source={})",
                    c.scheme,
                    c.port,
                    c.pid,
                    c.source
                );
                return Some(endpoint);
            }
            Err(e) => *last_err = e,
        }
    }
    None
}

/// 找到一个可用端点。
///
/// 两级发现策略:
/// 1. 优先读取 `~/.gemini/antigravity*/daemon/*.json`(Antigravity 2.34+ 不再在命令行暴露
///    `--csrf_token`,且直接给出了 `httpPort`/`httpsPort` 与 `csrfToken`,免去进程枚举开销);
/// 2. 未命中时回退到旧版进程命令行 `--csrf_token` + 监听端口枚举。
///
/// 对每个候选优先试明文 HTTP——无需放宽证书校验,最省事;都不通再试 HTTPS。
/// 用 `Heartbeat` 作探活(payload 最小)。
pub fn probe_endpoint() -> Result<LsEndpoint, String> {
    let agent = build_agent(PROBE_TIMEOUT);
    let mut seen = std::collections::HashSet::new();
    let mut last_err = String::new();

    let daemon_candidates = detect_daemon_candidates();
    if let Some(ep) = try_probe_candidates(&agent, &daemon_candidates, &mut seen, &mut last_err) {
        return Ok(ep);
    }

    let process_candidates = detect_process_candidates();
    if let Some(ep) = try_probe_candidates(&agent, &process_candidates, &mut seen, &mut last_err) {
        return Ok(ep);
    }

    if daemon_candidates.is_empty() && process_candidates.is_empty() {
        return Err(
            "未发现运行中的 Antigravity language server。Antigravity 的 token 用量不落盘，\
             需要 Antigravity 保持打开才能读取"
                .to_string(),
        );
    }

    Err(format!(
        "Antigravity language server 已在运行但 RPC 不可达: {}",
        last_err
    ))
}

// ========== 会话候选发现 ==========

/// 一个待扫描的会话
#[derive(Debug, Clone)]
struct SessionCandidate {
    cascade_id: String,
    /// 用于增量判断的路径(brain 会话目录 或 conversations/<id>.pb)
    sync_path: PathBuf,
    mtime_nanos: i64,
    /// 上轮从该会话取到的用量条数(session_log_sync.last_line_offset)
    prev_row_count: i64,
}

fn metadata_modified_nanos(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// 目录自身与其下(浅递归 depth 层)最新的一个 metadata。
///
/// 只看目录 mtime 不够——Windows 下仅直接子项增删才更新目录 mtime,而
/// Antigravity 每轮对话是往 `.system_generated/logs/transcript.jsonl` 追加内容。
fn newest_metadata_in(dir: &Path, depth: u32) -> Option<std::fs::Metadata> {
    let mut best = std::fs::metadata(dir).ok();
    if depth == 0 {
        return best;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let candidate = if path.is_dir() {
                newest_metadata_in(&path, depth - 1)
            } else {
                entry.metadata().ok()
            };
            if let Some(candidate) = candidate {
                let better = best
                    .as_ref()
                    .map(|b| metadata_modified_nanos(&candidate) > metadata_modified_nanos(b))
                    .unwrap_or(true);
                if better {
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

/// 会话目录的"最后活动时间"
fn session_mtime_nanos(dir: &Path, depth: u32) -> i64 {
    newest_metadata_in(dir, depth)
        .map(|m| metadata_modified_nanos(&m))
        .unwrap_or(0)
}

/// `~/.gemini` 下存在的 Antigravity 数据根
fn antigravity_data_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let gemini = home.join(".gemini");
    DATA_ROOTS
        .iter()
        .map(|name| gemini.join(name))
        .filter(|p| p.is_dir())
        .collect()
}

/// 判断目录名是否为标准的 36 字符 UUID（8-4-4-4-12 十六进制），用于跳过 `brain/tempmediaStorage` 等非会话目录。
fn is_valid_cascade_id(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (idx, &b) in bytes.iter().enumerate() {
        match idx {
            8 | 13 | 18 | 23 => {
                if b != b'-' {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

/// 枚举会话候选:`brain/<cascadeId>/` 目录名 与 `conversations/<cascadeId>.pb` 文件名。
///
/// 同一 cascadeId 可能同时出现在多个根下(如 2.0 与备份),按 mtime 取最新的一份。
fn scan_session_candidates() -> Vec<SessionCandidate> {
    let mut map: HashMap<String, SessionCandidate> = HashMap::new();
    let mut consider = |cascade_id: String, sync_path: PathBuf, mtime_nanos: i64| {
        if cascade_id.is_empty() {
            return;
        }
        map.entry(cascade_id.clone())
            .and_modify(|existing| {
                if mtime_nanos > existing.mtime_nanos {
                    existing.sync_path = sync_path.clone();
                    existing.mtime_nanos = mtime_nanos;
                }
            })
            .or_insert(SessionCandidate {
                cascade_id,
                sync_path,
                mtime_nanos,
                prev_row_count: 0,
            });
    };

    for root in antigravity_data_roots() {
        let brain = root.join("brain");
        if brain.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&brain) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    if !is_valid_cascade_id(name) {
                        continue;
                    }
                    let mtime = session_mtime_nanos(&path, 3);
                    consider(name.to_string(), path.clone(), mtime);
                }
            }
        }

        let conversations = root.join("conversations");
        if conversations.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&conversations) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file()
                        || path.extension().and_then(|e| e.to_str()) != Some("pb")
                    {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    let mtime = entry
                        .metadata()
                        .map(|m| metadata_modified_nanos(&m))
                        .unwrap_or(0);
                    consider(stem.to_string(), path.clone(), mtime);
                }
            }
        }
    }

    let mut out: Vec<SessionCandidate> = map.into_values().collect();
    out.sort_by(|a, b| a.cascade_id.cmp(&b.cascade_id));
    out
}

/// Antigravity 数据目录是否存在(前端据此决定是否显示扫描入口)
pub fn antigravity_source_dir_available() -> bool {
    !antigravity_data_roots().is_empty()
}

/// 实际存在的首个数据根,用于前端展示数据目录。
///
/// 不能直接返回 `~/.gemini/antigravity`:装的是 IDE 版时只有 antigravity-ide
/// 目录,那样会在设置面板显示一个并不存在的路径。
pub fn primary_data_root() -> Option<PathBuf> {
    antigravity_data_roots().into_iter().next()
}

/// 全部 Antigravity 数据根下最新的文件 metadata,供 refresh_database 的 Phase1 快检使用
pub fn latest_session_file_mtime() -> Option<std::fs::Metadata> {
    antigravity_data_roots()
        .into_iter()
        .filter_map(|root| newest_metadata_in(&root, 4))
        .max_by_key(metadata_modified_nanos)
}

// ========== RPC 响应解析 ==========

/// 一条生成记录(= 一次模型请求)
#[derive(Debug, Clone, PartialEq)]
struct UsageRow {
    /// 去重键;缺失时回退 messageId
    dedup_key: String,
    model: String,
    input_tokens: i64,
    /// 含 thinking:protojson 的 outputTokens = thinkingOutputTokens + responseOutputTokens
    output_tokens: i64,
    cache_read: i64,
    latency: i64,
    first_token_latency: i64,
    response_id: String,
    message_id: String,
}

/// protojson 把 int64 编码成字符串,同时容忍数字形式
fn json_i64(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        _ => 0,
    }
}

fn json_str(value: Option<&Value>) -> String {
    value
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// protojson Duration("1.234s")转毫秒
fn duration_ms(value: Option<&Value>) -> i64 {
    let text = json_str(value);
    if text.is_empty() {
        return 0;
    }
    text.trim_end_matches('s')
        .parse::<f64>()
        .map(|secs| (secs * 1000.0).round() as i64)
        .unwrap_or(0)
}

/// RFC3339 时间戳转 Unix 秒
fn rfc3339_to_epoch(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|dt| dt.timestamp())
}

/// 从一个 usage 对象取出 token 明细。
///
/// 实测字段(Antigravity 2.0 / gemini-3.8-flash):
/// `inputTokens` / `outputTokens` / `thinkingOutputTokens` / `responseOutputTokens`
/// / `cacheReadTokens`(仅命中缓存前缀时才出现) / `responseId` / `messageId`。
/// `inputTokens` **不含** cacheRead(观测到 input < cacheRead 的行),
/// 因此无需像 gemini / codex 那样反向扣减。
fn parse_usage(usage: &Value, model: &str, latency: i64, first_token_latency: i64) -> Option<UsageRow> {
    let response_id = json_str(usage.get("responseId"));
    let message_id = json_str(usage.get("messageId"));
    let dedup_key = if !response_id.is_empty() {
        response_id.clone()
    } else if !message_id.is_empty() {
        message_id.clone()
    } else {
        return None;
    };

    let input_tokens = json_i64(usage.get("inputTokens"));
    let cache_read = json_i64(usage.get("cacheReadTokens"));
    // outputTokens 已含 thinking;个别响应只给分项时回退为两者之和
    let output_tokens = {
        let total = json_i64(usage.get("outputTokens"));
        if total > 0 {
            total
        } else {
            json_i64(usage.get("thinkingOutputTokens"))
                + json_i64(usage.get("responseOutputTokens"))
        }
    };
    if input_tokens == 0 && output_tokens == 0 && cache_read == 0 {
        return None;
    }

    Some(UsageRow {
        dedup_key,
        model: model.to_string(),
        input_tokens,
        output_tokens,
        cache_read,
        latency,
        first_token_latency,
        response_id,
        message_id,
    })
}

/// 解析 `GetCascadeTrajectoryGeneratorMetadata` 响应。
///
/// 每个 generatorMetadata 条目对应一轮生成,其 `chatModel` 下有两处 usage:
/// - `usage`:最终生效的那次
/// - `retryInfos[].usage`:每次尝试(含失败重试,各自真实消耗了 token)
///
/// 取 retryInfos 以免漏掉重试消耗,为空时回退 `usage`。两处在无重试时内容相同,
/// 由 dedup_key 折叠。模型名优先取 `responseModel`(如 `gemini-3.8-flash`),
/// `model` 字段是 `MODEL_PLACEHOLDER_M318` 这类占位符,不能用于定价匹配。
fn parse_generator_metadata(payload: &Value) -> Vec<UsageRow> {
    let entries = payload
        .get("generatorMetadata")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for entry in entries {
        let Some(chat_model) = entry.get("chatModel") else {
            continue;
        };
        let model = {
            let response_model = json_str(chat_model.get("responseModel"));
            if response_model.is_empty() {
                json_str(chat_model.get("model"))
            } else {
                response_model
            }
        };
        let (latency, first_token_latency) = {
            let streaming = duration_ms(chat_model.get("streamingDuration"));
            let ttft = duration_ms(chat_model.get("timeToFirstToken"));
            (ttft + streaming, ttft)
        };

        let mut collected = Vec::new();
        if let Some(retries) = chat_model.get("retryInfos").and_then(|v| v.as_array()) {
            for retry in retries {
                if let Some(usage) = retry.get("usage") {
                    if let Some(row) = parse_usage(usage, &model, latency, first_token_latency) {
                        collected.push(row);
                    }
                }
            }
        }
        if collected.is_empty() {
            if let Some(usage) = chat_model.get("usage") {
                if let Some(row) = parse_usage(usage, &model, latency, first_token_latency) {
                    collected.push(row);
                }
            }
        }
        for row in collected {
            if seen.insert(row.dedup_key.clone()) {
                out.push(row);
            }
        }
    }
    out
}

/// 解析 `GetCascadeTrajectory` 响应,建立 responseId / messageId → epoch 秒的映射。
///
/// generatorMetadata 自身不带时间戳,轨迹的 `steps[].metadata.createdAt` 才有;
/// 两者通过 `metadata.modelUsage.responseId`(或 messageId)对应。
fn parse_trajectory_timestamps(payload: &Value) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    let steps = payload
        .get("trajectory")
        .and_then(|t| t.get("steps"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    for step in steps {
        let Some(metadata) = step.get("metadata") else {
            continue;
        };
        let Some(epoch) = rfc3339_to_epoch(&json_str(metadata.get("createdAt"))) else {
            continue;
        };
        let Some(model_usage) = metadata.get("modelUsage") else {
            continue;
        };
        for key in ["responseId", "messageId"] {
            let id = json_str(model_usage.get(key));
            if id.is_empty() {
                continue;
            }
            // 同一 id 多次出现时取最早的一次(轨迹可能因 fork 重复记录)
            map.entry(id)
                .and_modify(|current| {
                    if epoch < *current {
                        *current = epoch;
                    }
                })
                .or_insert(epoch);
        }
    }
    map
}

// ========== 扫描入口 ==========

/// mtime 游标能否推进到本次观测值。
///
/// RPC 取到的用量可能滞后于文件 mtime:流式响应进行中时 `transcript.jsonl` 已更新,
/// 而 `generatorMetadata` 里还没有这一轮。此时若照常推进游标,下轮就会因"mtime 未变"
/// 跳过该会话,这一轮用量将永久缺失(直到用户又发一条消息才被动补上)。
///
/// 所以只在两种情况下推进:
/// - 本次确实比上轮多取到了记录(数据已落到 language server 里)
/// - 会话已静置超过 [`RESCAN_GRACE_SECS`](认定不会再有新用量,否则活跃判据会让
///   "改了文件但永远不产生用量"的会话每轮都白跑两次 RPC)
///
/// 不推进时该会话下轮继续重扫,重复记录由 `request_id` 主键挡掉。
fn should_advance_cursor(
    row_count: usize,
    prev_row_count: i64,
    mtime_nanos: i64,
    now_epoch: i64,
) -> bool {
    if row_count as i64 > prev_row_count {
        return true;
    }
    now_epoch - mtime_nanos / 1_000_000_000 >= RESCAN_GRACE_SECS
}

/// 扫描一个会话:两次 RPC 取用量与时间戳,写入 session_request_logs。
///
/// 返回 (导入条数, 跳过条数)。时间戳缺失的记录回退到会话 mtime——宁可日期
/// 略有偏差也不丢记录,否则该轮用量会永久缺失。
fn scan_one_session(
    app_db: &AppDbService,
    agent: &ureq::Agent,
    endpoint: &LsEndpoint,
    candidate: &SessionCandidate,
) -> Result<(u32, u32), String> {
    let body = serde_json::json!({ "cascadeId": candidate.cascade_id });
    let metadata = rpc_call(agent, endpoint, "GetCascadeTrajectoryGeneratorMetadata", &body)?;
    let rows = parse_generator_metadata(&metadata);
    let advance_cursor = should_advance_cursor(
        rows.len(),
        candidate.prev_row_count,
        candidate.mtime_nanos,
        crate::utils::now_epoch_seconds(),
    );

    if rows.is_empty() {
        // 尚未产生用量(或 language server 重启后丢了会话状态)。静置后仍为空则
        // 记下游标,免得这类会话每轮都被重扫。
        if advance_cursor {
            let conn = app_db.conn();
            AppDbService::update_session_log_sync_on_conn(
                conn,
                ANTIGRAVITY_SOURCE,
                &candidate.sync_path.to_string_lossy(),
                candidate.mtime_nanos,
                0,
            )?;
        }
        return Ok((0, 0));
    }

    // 时间戳是可选增强:轨迹 RPC 失败不该让整个会话的用量丢掉
    let timestamps = match rpc_call(agent, endpoint, "GetCascadeTrajectory", &body) {
        Ok(trajectory) => parse_trajectory_timestamps(&trajectory),
        Err(e) => {
            log::warn!(
                "[ANTIGRAVITY] 会话 {} 时间戳获取失败,回退到文件 mtime: {}",
                candidate.cascade_id,
                e
            );
            HashMap::new()
        }
    };
    let fallback_epoch = candidate.mtime_nanos / 1_000_000_000;

    let conn = app_db.conn();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("开启事务失败: {}", e))?;
    let mut imported = 0u32;
    let mut skipped = 0u32;
    for row in &rows {
        let created_at = timestamps
            .get(&row.response_id)
            .or_else(|| timestamps.get(&row.message_id))
            .copied()
            .unwrap_or(fallback_epoch);
        let request_id = format!("{}:{}", ANTIGRAVITY_SOURCE, row.dedup_key);
        let changed = AppDbService::insert_session_log_on_conn(
            &tx,
            ANTIGRAVITY_SOURCE,
            &request_id,
            &candidate.cascade_id,
            &row.model,
            ANTIGRAVITY_SOURCE,
            row.input_tokens,
            row.output_tokens,
            row.cache_read,
            0, // Antigravity 不报告 cache 写入量
            created_at,
            row.latency,
            row.first_token_latency,
        )?;
        if changed {
            imported += 1;
        } else {
            skipped += 1;
        }
    }
    if advance_cursor {
        AppDbService::update_session_log_sync_on_conn(
            &tx,
            ANTIGRAVITY_SOURCE,
            &candidate.sync_path.to_string_lossy(),
            candidate.mtime_nanos,
            rows.len() as i64,
        )?;
    }
    tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;
    Ok((imported, skipped))
}

fn empty_result(app_db: &AppDbService) -> DshScanResult {
    DshScanResult {
        files_scanned: 0,
        imported: 0,
        skipped: 0,
        errors: 0,
        total_records: app_db
            .get_session_log_count(ANTIGRAVITY_SOURCE)
            .unwrap_or(0),
    }
}

/// 扫描全部有新活动的 Antigravity 会话。要求 Antigravity 正在运行。
///
/// 会话 mtime 全都未变时**在探测端点之前**就返回:进程枚举要起一次
/// PowerShell(数百毫秒级),不能让每轮刷新都付这个代价。
/// 后台采集入口:language server 不可达时按指数退避跳过(见 BACKOFF 注释)。
/// 手动扫描命令直接调 [`scan_antigravity`],不受退避限制。
pub fn scan_antigravity_with_backoff(app_db: &AppDbService) -> Result<DshScanResult, String> {
    if !backoff_allows_try() {
        return Ok(empty_result(app_db));
    }
    let result = scan_antigravity(app_db);
    match &result {
        Ok(_) => backoff_reset(),
        Err(e) if is_endpoint_unavailable(e) => backoff_record_failure(),
        Err(_) => {}
    }
    result
}

pub fn scan_antigravity(app_db: &AppDbService) -> Result<DshScanResult, String> {
    let candidates = scan_session_candidates();
    let total_candidates = candidates.len();
    let pending: Vec<SessionCandidate> = candidates
        .into_iter()
        .filter_map(|mut c| {
            match app_db
                .get_session_log_sync_state(ANTIGRAVITY_SOURCE, &c.sync_path.to_string_lossy())
            {
                Some((last_modified, last_row_count)) => {
                    if last_modified >= c.mtime_nanos {
                        return None;
                    }
                    c.prev_row_count = last_row_count;
                    Some(c)
                }
                None => Some(c),
            }
        })
        .collect();
    if pending.is_empty() {
        return Ok(empty_result(app_db));
    }

    let endpoint = probe_endpoint()?;
    let agent = build_agent(RPC_TIMEOUT);
    let mut files_scanned = 0u32;
    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;

    for candidate in &pending {
        files_scanned += 1;
        match scan_one_session(app_db, &agent, &endpoint, candidate) {
            Ok((added, dup)) => {
                imported += added;
                skipped += dup;
            }
            Err(e) => {
                errors += 1;
                log::warn!(
                    "[ANTIGRAVITY] 会话 {} 扫描失败: {}",
                    candidate.cascade_id,
                    e
                );
            }
        }
    }

    let total_records = app_db
        .get_session_log_count(ANTIGRAVITY_SOURCE)
        .unwrap_or(0);
    log::info!(
        "[ANTIGRAVITY] 扫描完成: 会话 {}/{}, 新增 {}, 重复 {}, 失败 {}, 累计 {}",
        files_scanned,
        total_candidates,
        imported,
        skipped,
        errors,
        total_records
    );
    Ok(DshScanResult {
        files_scanned,
        imported,
        skipped,
        errors,
        total_records,
    })
}

/// 已入库的 Antigravity 记录数(启动时据此决定是否注册数据源:
/// 有历史数据就让用户能看到,不必先手动扫一次)
pub fn imported_record_count(app_db: &AppDbService) -> i64 {
    app_db
        .get_session_log_count(ANTIGRAVITY_SOURCE)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_delay_grows_then_caps_at_60s() {
        assert_eq!(
            (1..=6).map(backoff_delay_secs).collect::<Vec<_>>(),
            vec![5, 10, 20, 40, 60, 60]
        );
    }

    #[test]
    fn backoff_blocks_after_failure_and_clears_on_reset() {
        backoff_reset();
        assert!(backoff_allows_try());
        backoff_record_failure();
        assert!(!backoff_allows_try(), "失败后应进入退避窗口");
        backoff_reset();
        assert!(backoff_allows_try());
    }

    #[test]
    fn endpoint_unavailable_matches_probe_errors_only() {
        assert!(is_endpoint_unavailable("未发现运行中的 Antigravity language server。x"));
        assert!(is_endpoint_unavailable(
            "Antigravity language server 已在运行但 RPC 不可达: timeout"
        ));
        assert!(!is_endpoint_unavailable("扫描失败: 数据库被锁"));
    }

    #[test]
    fn extract_flag_handles_space_and_equals() {
        let cl = "language_server.exe --csrf_token abc-123 --extension_server_port=63190";
        assert_eq!(extract_flag(cl, "--csrf_token").unwrap(), "abc-123");
        assert_eq!(
            extract_flag(cl, "--extension_server_port").unwrap(),
            "63190"
        );
        assert!(extract_flag(cl, "--missing").is_none());
    }

    #[test]
    fn is_antigravity_ls_requires_both_markers() {
        assert!(is_antigravity_ls(
            r"C:\Users\x\.antigravity\bin\language_server.exe --csrf_token t"
        ));
        // language server 但不是 Antigravity（如 Windsurf）
        assert!(!is_antigravity_ls(
            r"C:\Users\x\.codeium\windsurf\language_server.exe"
        ));
        // Antigravity 主进程但不是 language server
        assert!(!is_antigravity_ls(r"C:\Program Files\Antigravity\Antigravity.exe"));
    }

    #[test]
    fn parse_process_line_accepts_tab_and_space() {
        assert_eq!(
            parse_process_line("1234\tlanguage_server.exe --x").unwrap(),
            (1234, "language_server.exe --x".to_string())
        );
        assert_eq!(
            parse_process_line(" 987 /usr/bin/language_server --y").unwrap(),
            (987, "/usr/bin/language_server --y".to_string())
        );
        assert!(parse_process_line("").is_none());
        assert!(parse_process_line("notapid something").is_none());
    }

    #[test]
    fn json_i64_reads_protojson_string_encoded_int64() {
        let v = serde_json::json!({ "a": "15522", "b": 42, "c": "x" });
        assert_eq!(json_i64(v.get("a")), 15522);
        assert_eq!(json_i64(v.get("b")), 42);
        assert_eq!(json_i64(v.get("c")), 0);
        assert_eq!(json_i64(None), 0);
    }

    #[test]
    fn duration_ms_parses_protojson_duration() {
        let v = serde_json::json!({ "d": "1.234s", "z": "0s", "e": "" });
        assert_eq!(duration_ms(v.get("d")), 1234);
        assert_eq!(duration_ms(v.get("z")), 0);
        assert_eq!(duration_ms(v.get("e")), 0);
    }

    /// 取自本机真实响应的结构（token 值为字符串编码的 int64）
    fn sample_metadata() -> Value {
        serde_json::json!({
            "generatorMetadata": [{
                "chatModel": {
                    "model": "MODEL_PLACEHOLDER_M318",
                    "responseModel": "gemini-3.8-flash",
                    "streamingDuration": "2.5s",
                    "usage": {
                        "model": "MODEL_PLACEHOLDER_M318",
                        "inputTokens": "15522",
                        "outputTokens": "135",
                        "thinkingOutputTokens": "110",
                        "responseOutputTokens": "25",
                        "messageId": "bot-aaa",
                        "responseId": "resp-1"
                    },
                    "retryInfos": [{
                        "usage": {
                            "inputTokens": "15522",
                            "outputTokens": "135",
                            "thinkingOutputTokens": "110",
                            "responseOutputTokens": "25",
                            "messageId": "bot-aaa",
                            "responseId": "resp-1"
                        }
                    }]
                }
            }]
        })
    }

    #[test]
    fn parse_generator_metadata_dedups_usage_and_retry_infos() {
        let rows = parse_generator_metadata(&sample_metadata());
        // chatModel.usage 与 retryInfos[0].usage 是同一次请求，只能计一条
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.model, "gemini-3.8-flash");
        assert_eq!(row.input_tokens, 15522);
        assert_eq!(row.output_tokens, 135);
        assert_eq!(row.cache_read, 0);
        assert_eq!(row.latency, 2500);
        assert_eq!(row.dedup_key, "resp-1");
    }

    #[test]
    fn parse_generator_metadata_sums_ttft_and_streaming_duration() {
        let payload = serde_json::json!({
            "generatorMetadata": [{
                "chatModel": {
                    "responseModel": "gemini-3.8-flash",
                    "timeToFirstToken": "15.0s",
                    "streamingDuration": "0.5s",
                    "usage": { "inputTokens": "100", "outputTokens": "50", "responseId": "r-1" }
                }
            }]
        });
        let rows = parse_generator_metadata(&payload);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].latency, 15500);
    }

    #[test]
    fn parse_generator_metadata_counts_every_retry() {
        let payload = serde_json::json!({
            "generatorMetadata": [{
                "chatModel": {
                    "responseModel": "gemini-3.8-flash",
                    "usage": { "inputTokens": "200", "outputTokens": "20", "responseId": "r-2" },
                    "retryInfos": [
                        { "usage": { "inputTokens": "100", "outputTokens": "10", "responseId": "r-1" } },
                        { "usage": { "inputTokens": "200", "outputTokens": "20", "responseId": "r-2" } }
                    ]
                }
            }]
        });
        let rows = parse_generator_metadata(&payload);
        // 失败重试也真实消耗 token，两条都要保留
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.iter().map(|r| r.input_tokens).sum::<i64>(), 300);
    }

    #[test]
    fn parse_generator_metadata_keeps_cache_read_when_present() {
        let payload = serde_json::json!({
            "generatorMetadata": [{
                "chatModel": {
                    "responseModel": "gemini-3.8-flash",
                    "usage": {
                        "inputTokens": "15508",
                        "outputTokens": "214",
                        "cacheReadTokens": "32677",
                        "responseId": "r-cache"
                    }
                }
            }]
        });
        let rows = parse_generator_metadata(&payload);
        assert_eq!(rows.len(), 1);
        // input < cacheRead，可见 inputTokens 不含缓存读取，不能反向扣减
        assert_eq!(rows[0].input_tokens, 15508);
        assert_eq!(rows[0].cache_read, 32677);
    }

    #[test]
    fn parse_generator_metadata_skips_placeholder_model_only_when_response_model_missing() {
        let payload = serde_json::json!({
            "generatorMetadata": [{
                "chatModel": {
                    "model": "MODEL_PLACEHOLDER_M318",
                    "usage": { "inputTokens": "1", "outputTokens": "1", "responseId": "r" }
                }
            }]
        });
        let rows = parse_generator_metadata(&payload);
        assert_eq!(rows[0].model, "MODEL_PLACEHOLDER_M318");
    }

    #[test]
    fn parse_generator_metadata_ignores_empty_and_identityless_usage() {
        let payload = serde_json::json!({
            "generatorMetadata": [
                { "chatModel": { "usage": { "inputTokens": "0", "outputTokens": "0", "responseId": "r" } } },
                { "chatModel": { "usage": { "inputTokens": "5", "outputTokens": "5" } } },
                { "notChatModel": {} }
            ]
        });
        assert!(parse_generator_metadata(&payload).is_empty());
    }

    #[test]
    fn cursor_advances_when_new_rows_arrived() {
        let now = 1_800_000_000;
        let fresh_mtime = (now - 5) * 1_000_000_000;
        // 比上轮多取到记录 → 数据已落到 language server，可以推进
        assert!(should_advance_cursor(19, 12, fresh_mtime, now));
        assert!(should_advance_cursor(1, 0, fresh_mtime, now));
    }

    #[test]
    fn cursor_holds_for_active_session_without_new_rows() {
        let now = 1_800_000_000;
        let fresh_mtime = (now - 5) * 1_000_000_000;
        // 文件刚变但 RPC 没有新用量（响应正在生成中）：必须保持游标，否则这一轮会丢
        assert!(!should_advance_cursor(12, 12, fresh_mtime, now));
        assert!(!should_advance_cursor(0, 0, fresh_mtime, now));
        // language server 重启后条数反而变少，同样不能推进
        assert!(!should_advance_cursor(3, 12, fresh_mtime, now));
    }

    #[test]
    fn cursor_advances_once_session_is_settled() {
        let now = 1_800_000_000;
        let stale_mtime = (now - RESCAN_GRACE_SECS) * 1_000_000_000;
        // 静置足够久仍无新用量 → 认定已稳定，推进游标，避免每轮白跑两次 RPC
        assert!(should_advance_cursor(12, 12, stale_mtime, now));
        assert!(should_advance_cursor(0, 0, stale_mtime, now));
        // 边界内侧仍然保持
        assert!(!should_advance_cursor(
            12,
            12,
            (now - RESCAN_GRACE_SECS + 1) * 1_000_000_000,
            now
        ));
    }

    /// 用真实的 insert / sync 语义验证「增量 + 去重」闭环：
    /// 同一批用量重复入库不会双计，新增条目按 responseId 追加。
    #[test]
    fn repeated_import_dedups_by_response_id() {
        let db = crate::services::app_db::AppDbService::new_in_memory().unwrap();
        let rows = parse_generator_metadata(&sample_metadata());
        assert_eq!(rows.len(), 1);

        let insert = |rows: &[UsageRow]| {
            let conn = db.conn();
            let tx = conn.unchecked_transaction().unwrap();
            let mut imported = 0;
            for row in rows {
                if crate::services::app_db::AppDbService::insert_session_log_on_conn(
                    &tx,
                    ANTIGRAVITY_SOURCE,
                    &format!("{}:{}", ANTIGRAVITY_SOURCE, row.dedup_key),
                    "cascade-a",
                    &row.model,
                    ANTIGRAVITY_SOURCE,
                    row.input_tokens,
                    row.output_tokens,
                    row.cache_read,
                    0,
                    1_000,
                    row.latency,
                    row.first_token_latency,
                )
                .unwrap()
                {
                    imported += 1;
                }
            }
            tx.commit().unwrap();
            imported
        };

        assert_eq!(insert(&rows), 1);
        // 重扫同一会话（游标未推进的情况）不应产生第二条
        assert_eq!(insert(&rows), 0);
        assert_eq!(db.get_session_log_count(ANTIGRAVITY_SOURCE).unwrap(), 1);

        // 新一轮对话：新 responseId 追加，旧记录保持
        let next = parse_generator_metadata(&serde_json::json!({
            "generatorMetadata": [{
                "chatModel": {
                    "responseModel": "gemini-3.8-flash",
                    "usage": { "inputTokens": "700", "outputTokens": "70", "responseId": "resp-2" }
                }
            }]
        }));
        assert_eq!(insert(&next), 1);
        assert_eq!(db.get_session_log_count(ANTIGRAVITY_SOURCE).unwrap(), 2);
    }

    /// 游标写入后能被读回，且未变化的 mtime 会让会话被跳过
    #[test]
    fn sync_cursor_roundtrip_marks_session_scanned() {
        let db = crate::services::app_db::AppDbService::new_in_memory().unwrap();
        let path = r"C:\Users\x\.gemini\antigravity\brain\cascade-a";
        assert!(db
            .get_session_log_sync_state(ANTIGRAVITY_SOURCE, path)
            .is_none());

        crate::services::app_db::AppDbService::update_session_log_sync_on_conn(
            db.conn(),
            ANTIGRAVITY_SOURCE,
            path,
            5_000_000_000,
            19,
        )
        .unwrap();

        let (mtime, count) = db
            .get_session_log_sync_state(ANTIGRAVITY_SOURCE, path)
            .unwrap();
        assert_eq!((mtime, count), (5_000_000_000, 19));
        // pending 判据：mtime 未前进 → 跳过；前进 → 重扫并带上上轮条数
        assert!(mtime >= 5_000_000_000);
        assert!(mtime < 6_000_000_000);
    }

    #[test]
    fn parse_trajectory_timestamps_maps_both_identities() {
        let payload = serde_json::json!({
            "trajectory": { "steps": [
                { "metadata": { "createdAt": "2026-09-16T03:11:55.593842700Z",
                                "modelUsage": { "responseId": "resp-1", "messageId": "bot-aaa" } } },
                { "metadata": { "createdAt": "2026-09-16T03:12:08Z" } }
            ]}
        });
        let map = parse_trajectory_timestamps(&payload);
        assert_eq!(map.get("resp-1"), Some(&1789528315));
        assert_eq!(map.get("bot-aaa"), Some(&1789528315));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn parse_trajectory_timestamps_prefers_earliest_for_forked_steps() {
        let payload = serde_json::json!({
            "trajectory": { "steps": [
                { "metadata": { "createdAt": "2026-09-16T03:35:00Z",
                                "modelUsage": { "responseId": "r" } } },
                { "metadata": { "createdAt": "2026-09-16T03:34:00Z",
                                "modelUsage": { "responseId": "r" } } }
            ]}
        });
        let map = parse_trajectory_timestamps(&payload);
        assert_eq!(map.get("r"), Some(&rfc3339_to_epoch("2026-09-16T03:34:00Z").unwrap()));
    }

    #[test]
    fn parse_daemon_descriptor_extracts_ports_pid_and_token() {
        let raw = r#"{
            "pid": 26348,
            "httpsPort": 57438,
            "httpPort": 57439,
            "lsVersion": "2.34.0",
            "csrfToken": "tok-abc-123"
        }"#;
        let desc = parse_daemon_descriptor(raw).unwrap();
        assert_eq!(desc.pid, 26348);
        assert_eq!(desc.http_port, Some(57439));
        assert_eq!(desc.https_port, Some(57438));
        assert_eq!(desc.csrf_token, "tok-abc-123");

        // 只有单一端口且忽略 0 端口
        let https_only = r#"{"pid": 99, "httpPort": 0, "httpsPort": 50001, "csrfToken": "t"}"#;
        let desc = parse_daemon_descriptor(https_only).unwrap();
        assert_eq!(desc.http_port, None);
        assert_eq!(desc.https_port, Some(50001));

        // 缺少 token 或无有效端口均应丢弃
        assert!(parse_daemon_descriptor(r#"{"pid": 1, "httpPort": 8080, "csrfToken": ""}"#).is_none());
        assert!(parse_daemon_descriptor(r#"{"pid": 1, "httpPort": 0, "httpsPort": 0, "csrfToken": "t"}"#).is_none());
        assert!(parse_daemon_descriptor("not json").is_none());
    }

    #[test]
    fn collect_daemon_candidates_filters_dead_pids_and_orders_http_before_https() {
        let dir = tempfile::tempdir().unwrap();
        let daemon = dir.path().join("daemon");
        std::fs::create_dir_all(&daemon).unwrap();

        // 已退出 PID 的残留文件应被跳过
        std::fs::write(
            daemon.join("hub_52176.json"),
            r#"{"pid": 17400, "httpPort": 52176, "httpsPort": 52175, "csrfToken": "dead-tok"}"#,
        )
        .unwrap();
        // 存活 PID 的有效文件
        std::fs::write(
            daemon.join("hub_57439.json"),
            r#"{"pid": 26348, "httpPort": 57439, "httpsPort": 57438, "csrfToken": "live-tok"}"#,
        )
        .unwrap();
        // 非 json 文件与损坏 json 应静默跳过
        std::fs::write(daemon.join("notes.txt"), "ignored").unwrap();
        std::fs::write(daemon.join("broken.json"), "{broken").unwrap();

        let candidates =
            collect_daemon_candidates_in(&[dir.path().to_path_buf()], |pid| pid == 26348);
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates[0],
            EndpointCandidate {
                pid: 26348,
                scheme: "http",
                port: 57439,
                csrf_token: "live-tok".to_string(),
                source: "daemon",
            }
        );
        assert_eq!(
            candidates[1],
            EndpointCandidate {
                pid: 26348,
                scheme: "https",
                port: 57438,
                csrf_token: "live-tok".to_string(),
                source: "daemon",
            }
        );
    }

    #[test]
    fn is_pid_alive_detects_current_process_and_rejects_zero() {
        assert!(is_pid_alive(std::process::id()));
        assert!(!is_pid_alive(0));
    }

    #[test]
    fn is_valid_cascade_id_accepts_uuid_and_rejects_non_session_dirs() {
        assert!(is_valid_cascade_id("460aa64e-335e-4c60-a4cc-119a1987072f"));
        assert!(is_valid_cascade_id("01E4658B-46BB-4667-9D34-6D1A814EE6BE"));
        assert!(!is_valid_cascade_id("tempmediaStorage"));
        assert!(!is_valid_cascade_id(""));
        assert!(!is_valid_cascade_id("460aa64e-335e-4c60-a4cc-119a1987072"));
        assert!(!is_valid_cascade_id("460aa64e_335e_4c60_a4cc_119a1987072f"));
        assert!(!is_valid_cascade_id("460aa64e-335e-4c60-a4cc-119a1987072g"));
    }

    #[test]
    #[ignore = "需要本机正在运行 Antigravity 才能执行真机探活"]
    fn probe_and_scan_real_antigravity_in_memory() {
        let ep = probe_endpoint().expect("本机运行中的 Antigravity 应可探活");
        assert!(ep.port > 0);
        assert!(!ep.csrf_token.is_empty());

        let db = crate::services::app_db::AppDbService::new_in_memory().unwrap();
        let result = scan_antigravity(&db).expect("真机扫描应成功");
        eprintln!(
            "real probe ok: {}://127.0.0.1:{}, scanned={}, imported={}, errors={}",
            ep.scheme, ep.port, result.files_scanned, result.imported, result.errors
        );
        assert!(result.files_scanned > 0);
        assert!(result.imported > 0);
    }
}
