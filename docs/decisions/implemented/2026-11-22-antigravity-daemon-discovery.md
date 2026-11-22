# DR: Antigravity language server 优先从 daemon 描述文件发现

Status: implemented

## Problem

Antigravity 2.34.0 起 `language_server` 进程不再在命令行参数中传递 `--csrf_token`，改为将运行时连接信息（`pid`、`httpPort`、`httpsPort`、`lsVersion`、`csrfToken`）写入 `~/.gemini/antigravity*/daemon/hub_<httpPort>.json`。原有发现逻辑依赖枚举进程命令行并强制提取 `--csrf_token`，导致新版 Antigravity 运行时全部候选被跳过，误报“未发现运行中的 Antigravity language server”，用量采集完全中断。

## Decision

在 `antigravity_scanner::probe_endpoint` 中采用两级发现策略：

1. **优先读取 `daemon/*.json`**：遍历 `~/.gemini/{antigravity,antigravity-ide,antigravity-backup}/daemon/*.json`，解析 `pid`、`httpPort`、`httpsPort` 与 `csrfToken`；用平台原生 PID 存活检查（Windows `OpenProcess` + `GetExitCodeProcess`，Unix `kill(pid, 0)`）过滤历史退出实例残留的旧文件，按文件修改时间降序排列，每个存活实例先试 `httpPort`（明文 HTTP）再试 `httpsPort`（HTTPS），用 `Heartbeat` 探活。
2. **回退旧版命令行发现**：仅当 `daemon/*.json` 未找到可用端点时，回退至原有的进程命令行 `--csrf_token` 与监听端口枚举路径，对已探测过的 `(scheme, port, csrf_token)` 去重。

实测确认 Antigravity 2.34.0 的 `Heartbeat`、`GetCascadeTrajectoryGeneratorMetadata` 与 `GetCascadeTrajectory` 响应结构与既有解析完全兼容，保留 RPC 作为唯一数据来源，延续 [退避策略](2026-10-25-antigravity-unreachable-backoff.md) 的错误前缀约定。

## Alternatives considered

- **改用 `transcript.jsonl` 离线直扫** —— 新版 `transcript.jsonl` 虽已包含 `input_tokens` / `output_tokens` / `cache_read_tokens` / `model`，但缺失 `latency`、`first_token_latency`、失败重试消耗（`retryInfos`）及唯一 `responseId`，不仅丢失性能指标，还与已入库的 `Antigravity:<responseId>` 去重口径冲突，且旧会话不含 token 字段。
- **不过滤 PID 直接对全部 `daemon/*.json` 探活** —— `daemon/` 会长期残留历史退出或崩溃实例的旧文件；在 Windows 上向未监听的回环端口发起 TCP 连接会触发秒级重试等待，Antigravity 未运行时会显著拖慢扫描。
- **直接删除旧版命令行 `--csrf_token` 发现路径** —— 旧版 Antigravity 或 IDE 变体可能未生成 `daemon/*.json`，保留作为次级回退无额外运行时开销（命中 daemon 时根本不触发），向后兼容性更好。

## Consequences

- 换来：恢复 Antigravity 2.34.0+ 的实时与手动扫描采集；正常运行时命中 `daemon/*.json` 首跳即通，免去 Windows 下起 PowerShell 枚举进程与端口的数百毫秒开销；完整保留耗时、首字延迟、重试消耗与原有去重键。
- 代价：引入少量无外部依赖的平台原生 PID 存活检测 FFI 代码；依赖 `daemon/*.json` 的字段约定，若未来 Antigravity 调整描述文件位置或字段名仍需跟进适配。
