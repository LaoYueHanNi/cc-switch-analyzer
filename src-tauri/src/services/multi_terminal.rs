//! 多终端布局引擎
//!
//! 负责把 N 个 PaneSpec 拼成 Windows Terminal (`wt`) 的命令行参数列表。
//! 每个 tab 内部最多 4 个 pane(2×2 网格),超过 4 个自动开新 tab。
//!
//! 设计:布局引擎只关心"分几个 pane / 怎么切",不关心具体跑什么命令 ——
//! 命令拼装由调用方通过 `build_cmd` 闭包注入,这样单元测试可以验证布局
//! 而不必 mock 真实的 agent 启动器。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    OpenCode,
    Codex,
}

/// 单个 pane 的描述。`session_id` 为 None 表示"开新会话"(本轮未使用)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSpec {
    pub agent: AgentKind,
    pub session_id: Option<String>,
    pub project_dir: Option<String>,
}

/// 把单个 pane 拼成 PowerShell `-Command` 后面的命令字符串
/// (不含 `powershell` / `-NoExit` / `-Command` 关键字)。
///
/// 返回的字符串**不能含 `;`**,否则 `wt` 会把它当成子命令分隔符。
pub fn build_pane_command(spec: &PaneSpec) -> String {
    match (&spec.agent, &spec.session_id) {
        (AgentKind::Claude, Some(sid)) => format!("claude --resume {}", sid),
        (AgentKind::OpenCode, Some(sid)) => format!("opencode -s {}", sid),
        (AgentKind::Codex, Some(sid)) => format!("codex resume {}", sid),
        // 兜底:开新会话(本轮不用,但保留能力)
        (AgentKind::Claude, None) => "claude".to_string(),
        (AgentKind::OpenCode, None) => "opencode".to_string(),
        (AgentKind::Codex, None) => "codex".to_string(),
    }
}

/// 把数据库里 `task_sessions.source` 字段的字符串映射到 `AgentKind`。
///
/// 重要:这里写的是**数据库实际存的字符串**,不是前端/UI 上的名字。
/// 历史原因数据库用 `claudecode`(`app_db.rs:1555`、`session_title.rs:103`),
/// 不是 `claude`。改这个之前先 grep 数据库看看。
pub fn agent_kind_from_source(source: &str) -> Option<AgentKind> {
    match source {
        "claudecode" => Some(AgentKind::Claude),
        "opencode" => Some(AgentKind::OpenCode),
        "codex" => Some(AgentKind::Codex),
        _ => None,
    }
}

/// 单个 pane 在 `wt` 参数列表中的展开
///
/// 形如:
///   `["powershell", "-NoExit", "-Command", "<cmd>"]`
///
/// 若 `project_dir` 非空,在前面插入 `["-d", "<dir>"]`。
fn pane_args(spec: &PaneSpec, cmd: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(7);
    if let Some(dir) = &spec.project_dir {
        if !dir.is_empty() {
            out.push("-d".to_string());
            out.push(dir.clone());
        }
    }
    out.push("powershell".to_string());
    out.push("-NoExit".to_string());
    out.push("-Command".to_string());
    out.push(cmd.to_string());
    out
}

/// 单个 tab 内部的布局(最多 4 个 pane)
///
/// n=1: 1 个 `nt` + 首 pane
/// n=2: `nt + sp -V`
/// n=3: `nt + sp -V + sp -H`
/// n=4: `nt + sp -V + sp -H + move-focus left + sp -H`
///
/// `first_tab_keyword` 是 tab 内部第一个命令的关键字:
/// - 第一个 tab 用 `"nt"`(new-tab 的子命令)
/// - 后续 tab 在 `; new-tab` 之后直接跟命令,**不要再加 `nt`**(否则 `wt`
///   会在新 tab 里又起一个 pane,而新 pane 没有 `-d` 之类的配置,
///   会落在 `wt` 进程的默认工作目录,即用户 home)
fn tab_args(
    tab_panes: &[PaneSpec],
    build_cmd: &dyn Fn(&PaneSpec) -> String,
    first_tab_keyword: &str,
) -> Vec<String> {
    let n = tab_panes.len();
    if n == 0 {
        return vec![];
    }

    let mut out: Vec<String> = Vec::new();
    if !first_tab_keyword.is_empty() {
        out.push(first_tab_keyword.to_string());
    }
    out.extend(pane_args(&tab_panes[0], &build_cmd(&tab_panes[0])));

    if n >= 2 {
        out.push(";".to_string());
        out.push("sp".to_string());
        out.push("-V".to_string());
        out.push("-s".to_string());
        out.push("0.5".to_string());
        out.extend(pane_args(&tab_panes[1], &build_cmd(&tab_panes[1])));
    }

    if n >= 3 {
        out.push(";".to_string());
        out.push("sp".to_string());
        out.push("-H".to_string());
        out.push("-s".to_string());
        out.push("0.5".to_string());
        out.extend(pane_args(&tab_panes[2], &build_cmd(&tab_panes[2])));
    }

    if n == 4 {
        out.push(";".to_string());
        out.push("move-focus".to_string());
        out.push("left".to_string());
        out.push(";".to_string());
        out.push("sp".to_string());
        out.push("-H".to_string());
        out.push("-s".to_string());
        out.push("0.5".to_string());
        out.extend(pane_args(&tab_panes[3], &build_cmd(&tab_panes[3])));
    }

    out
}

/// 给定 N 个 pane,返回拼好的 `wt` 参数列表
///
/// 自动按 4 一组切 tab,余数 <4 单独开 tab。
/// n=0 时返回空(防御性)。
pub fn build_wt_args<F>(panes: &[PaneSpec], build_cmd: F) -> Vec<String>
where
    F: Fn(&PaneSpec) -> String,
{
    if panes.is_empty() {
        return vec![];
    }

    let mut out: Vec<String> = vec!["-w".to_string(), "0".to_string()];

    let mut first_tab = true;
    for chunk in panes.chunks(4) {
        if !first_tab {
            // 不是第一个 tab,在前面加分隔符 + new-tab
            out.push(";".to_string());
            out.push("new-tab".to_string());
        }
        // 第一个 tab 用 "nt" 作为首个命令,后续 tab 在 "new-tab" 之后直接跟命令
        let keyword = if first_tab { "nt" } else { "" };
        first_tab = false;
        out.extend(tab_args(&chunk, &build_cmd, keyword));
    }

    out
}

// ==================== 单元测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_claude(sid: &str) -> PaneSpec {
        PaneSpec {
            agent: AgentKind::Claude,
            session_id: Some(sid.to_string()),
            project_dir: Some("/tmp/proj".to_string()),
        }
    }

    fn make_opencode(sid: &str) -> PaneSpec {
        PaneSpec {
            agent: AgentKind::OpenCode,
            session_id: Some(sid.to_string()),
            project_dir: Some("/tmp/proj".to_string()),
        }
    }

    fn make_codex(sid: &str) -> PaneSpec {
        PaneSpec {
            agent: AgentKind::Codex,
            session_id: Some(sid.to_string()),
            project_dir: None,
        }
    }

    /// 简单的"echo sid"占位 build_cmd,只看布局不关心命令
    fn echo_cmd(spec: &PaneSpec) -> String {
        format!("echo {}", spec.session_id.as_deref().unwrap_or("?"))
    }

    // ----- build_pane_command -----

    #[test]
    fn test_build_pane_command_claude() {
        let spec = PaneSpec {
            agent: AgentKind::Claude,
            session_id: Some("ses_abc123".to_string()),
            project_dir: None,
        };
        assert_eq!(build_pane_command(&spec), "claude --resume ses_abc123");
    }

    #[test]
    fn test_build_pane_command_opencode() {
        let spec = PaneSpec {
            agent: AgentKind::OpenCode,
            session_id: Some("ses_def456".to_string()),
            project_dir: None,
        };
        assert_eq!(build_pane_command(&spec), "opencode -s ses_def456");
    }

    #[test]
    fn test_build_pane_command_codex() {
        let spec = PaneSpec {
            agent: AgentKind::Codex,
            session_id: Some("ses_ghi789".to_string()),
            project_dir: None,
        };
        assert_eq!(build_pane_command(&spec), "codex resume ses_ghi789");
    }

    #[test]
    fn test_build_pane_command_session_id_with_special_chars() {
        let spec = PaneSpec {
            agent: AgentKind::Claude,
            session_id: Some("abc-123_XYZ.999".to_string()),
            project_dir: None,
        };
        // session_id 不需要引号包,实际跑的时候是 `claude --resume abc-123_XYZ.999`
        let cmd = build_pane_command(&spec);
        assert_eq!(cmd, "claude --resume abc-123_XYZ.999");
        assert!(!cmd.contains(';'), "命令不能含 `;`");
    }

    #[test]
    fn test_build_pane_command_no_session_id_opens_new() {
        let spec = PaneSpec {
            agent: AgentKind::Claude,
            session_id: None,
            project_dir: None,
        };
        assert_eq!(build_pane_command(&spec), "claude");
    }

    // ----- build_wt_args: 0~4 pane -----

    #[test]
    fn test_build_wt_args_empty() {
        let args = build_wt_args(&[], echo_cmd);
        assert!(args.is_empty(), "空列表应返回空 args,实际: {:?}", args);
    }

    #[test]
    fn test_build_wt_args_one_pane() {
        let panes = vec![make_claude("a")];
        let args = build_wt_args(&panes, echo_cmd);
        assert_eq!(
            args,
            vec![
                "-w", "0",
                "nt", "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo a",
            ]
        );
    }

    #[test]
    fn test_build_wt_args_two_panes() {
        let panes = vec![make_claude("a"), make_opencode("b")];
        let args = build_wt_args(&panes, echo_cmd);
        assert_eq!(
            args,
            vec![
                "-w", "0",
                "nt", "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo a",
                ";", "sp", "-V", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo b",
            ]
        );
    }

    #[test]
    fn test_build_wt_args_three_panes() {
        let panes = vec![make_claude("a"), make_opencode("b"), make_codex("c")];
        let args = build_wt_args(&panes, echo_cmd);
        assert_eq!(
            args,
            vec![
                "-w", "0",
                "nt", "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo a",
                ";", "sp", "-V", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo b",
                ";", "sp", "-H", "-s", "0.5",
                    "powershell", "-NoExit", "-Command", "echo c",
                // 注意:make_codex 的 project_dir=None,所以没有 -d
            ]
        );
    }

    #[test]
    fn test_build_wt_args_four_panes_2x2() {
        let panes = vec![
            make_claude("a"),
            make_opencode("b"),
            make_codex("c"),
            make_claude("d"),
        ];
        let args = build_wt_args(&panes, echo_cmd);
        assert_eq!(
            args,
            vec![
                "-w", "0",
                "nt", "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo a",
                ";", "sp", "-V", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo b",
                ";", "sp", "-H", "-s", "0.5",
                    "powershell", "-NoExit", "-Command", "echo c",
                ";", "move-focus", "left",
                ";", "sp", "-H", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo d",
            ]
        );
    }

    // ----- build_wt_args: 5~9 pane(多 tab) -----

    #[test]
    fn test_build_wt_args_five_panes_two_tabs() {
        let panes = vec![
            make_claude("a"), make_claude("b"), make_claude("c"), make_claude("d"),
            make_claude("e"),
        ];
        let args = build_wt_args(&panes, echo_cmd);
        // tab 1: 4 个 pane(2×2)
        // tab 2: 1 个 pane(全占)
        assert_eq!(
            args,
            vec![
                "-w", "0",
                "nt", "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo a",
                ";", "sp", "-V", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo b",
                ";", "sp", "-H", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo c",
                ";", "move-focus", "left",
                ";", "sp", "-H", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo d",
                ";", "new-tab", "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo e",
            ]
        );
    }

    #[test]
    fn test_build_wt_args_six_panes_two_tabs_4_plus_2() {
        let panes = vec![
            make_claude("a"), make_claude("b"), make_claude("c"), make_claude("d"),
            make_claude("e"), make_claude("f"),
        ];
        let args = build_wt_args(&panes, echo_cmd);
        // tab 1: 4 个 pane(2×2)
        // tab 2: 2 个 pane(主左+右)
        assert_eq!(
            args,
            vec![
                "-w", "0",
                "nt", "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo a",
                ";", "sp", "-V", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo b",
                ";", "sp", "-H", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo c",
                ";", "move-focus", "left",
                ";", "sp", "-H", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo d",
                ";", "new-tab", "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo e",
                ";", "sp", "-V", "-s", "0.5",
                    "-d", "/tmp/proj", "powershell", "-NoExit", "-Command", "echo f",
            ]
        );
    }

    #[test]
    fn test_build_wt_args_eight_panes_two_tabs_both_2x2() {
        let panes = vec![
            make_claude("a"), make_claude("b"), make_claude("c"), make_claude("d"),
            make_claude("e"), make_claude("f"), make_claude("g"), make_claude("h"),
        ];
        let args = build_wt_args(&panes, echo_cmd);
        // 两个 tab 都是 2×2
        assert_eq!(args.len(), args.len()); // 防止编译器优化掉
        // 数 "; new-tab" 出现次数:1(分隔两个 tab)
        let newtab_count = args
            .iter()
            .enumerate()
            .filter(|(i, s)| *s == "new-tab" && i.checked_sub(1).and_then(|j| args.get(j)) == Some(&";".to_string()))
            .count();
        assert_eq!(newtab_count, 1, "应该只有 1 个 new-tab 分隔,实际 args: {:?}", args);
        // 验证 tab 2 也是 2×2:从 "new-tab" 之后开始,数 "sp" 出现次数
        let after_newtab = args
            .iter()
            .position(|s| s == "new-tab")
            .expect("应该能找到 new-tab");
        let tail = &args[after_newtab + 1..];
        let sp_count = tail.iter().filter(|s| *s == "sp").count();
        assert_eq!(sp_count, 3, "tab 2 是 2×2 应该 3 次 sp,实际 tail: {:?}", tail);
        // move-focus 在 tab 2 也应该出现 1 次(2×2 必须)
        let mf_count = tail.iter().filter(|s| *s == "move-focus").count();
        assert_eq!(mf_count, 1, "tab 2 应该 1 次 move-focus");
    }

    #[test]
    fn test_build_wt_args_nine_panes_three_tabs() {
        let panes: Vec<_> = (1..=9).map(|i| make_claude(&format!("p{}", i))).collect();
        let args = build_wt_args(&panes, echo_cmd);
        // tab 1: 2×2, tab 2: 2×2, tab 3: 1 个全占
        let newtab_count = args
            .iter()
            .enumerate()
            .filter(|(i, s)| *s == "new-tab" && i.checked_sub(1).and_then(|j| args.get(j)) == Some(&";".to_string()))
            .count();
        assert_eq!(newtab_count, 2, "9 个 pane 应该开 2 次 new-tab,实际 args: {:?}", args);
    }

    // ----- 验证 build_cmd 注入确实生效 -----

    #[test]
    fn test_build_cmd_injection_works() {
        let panes = vec![make_claude("sid-001")];
        let args = build_wt_args(&panes, build_pane_command);
        // 最后一段应该是 "claude --resume sid-001"
        let cmd_str = args.last().expect("args 不应为空");
        assert_eq!(cmd_str, "claude --resume sid-001");
    }

    #[test]
    fn test_no_semicolon_in_command_output() {
        // 防止 sessionId 含分号这种奇葩 case 误把命令切断
        let panes: Vec<_> = (1..=16)
            .map(|i| PaneSpec {
                agent: if i % 3 == 0 {
                    AgentKind::Codex
                } else if i % 3 == 1 {
                    AgentKind::Claude
                } else {
                    AgentKind::OpenCode
                },
                session_id: Some(format!("ses-{}", i)),
                project_dir: Some("/tmp".to_string()),
            })
            .collect();
        let args = build_wt_args(&panes, build_pane_command);
        // 所有 "-Command" 后面紧跟的那个元素
        for (i, s) in args.iter().enumerate() {
            if s == "-Command" {
                if let Some(cmd) = args.get(i + 1) {
                    assert!(
                        !cmd.contains(';'),
                        "Command 后面的字符串不能含 `;`,实际: {}",
                        cmd
                    );
                }
            }
        }
    }

    /// 防止以后改 source 字符串时(数据库存的是 "claudecode" 而不是 "claude")
    /// 又把这个对应关系改错。pane 用的命令前缀要跟 AgentKind 匹配。
    #[test]
    fn test_pane_command_matches_agent_kind() {
        // Claude → 跑 "claude" 可执行
        let claude = PaneSpec {
            agent: AgentKind::Claude,
            session_id: Some("abc".to_string()),
            project_dir: None,
        };
        assert!(build_pane_command(&claude).starts_with("claude"));

        // OpenCode → "opencode"
        let oc = PaneSpec {
            agent: AgentKind::OpenCode,
            session_id: Some("abc".to_string()),
            project_dir: None,
        };
        assert!(build_pane_command(&oc).starts_with("opencode"));

        // Codex → "codex"
        let cx = PaneSpec {
            agent: AgentKind::Codex,
            session_id: Some("abc".to_string()),
            project_dir: None,
        };
        assert!(build_pane_command(&cx).starts_with("codex"));
    }

    /// 回归测试:确保数据库存的 source 字符串(是 "claudecode" 不是 "claude")
    /// 能正确映射到 AgentKind。历史 bug:`open_task_sessions` 写成
    /// `"claude" => AgentKind::Claude`,但数据库里是 `"claudecode"`,导致
    /// 所有 Claude session 都被跳过。
    #[test]
    fn test_agent_kind_from_source_claudecode() {
        assert_eq!(agent_kind_from_source("claudecode"), Some(AgentKind::Claude));
        assert_eq!(agent_kind_from_source("opencode"), Some(AgentKind::OpenCode));
        assert_eq!(agent_kind_from_source("codex"), Some(AgentKind::Codex));
        assert_eq!(agent_kind_from_source("claude"), None); // 故意拒绝短写
        assert_eq!(agent_kind_from_source(""), None);
        assert_eq!(agent_kind_from_source("unknown"), None);
    }


}
