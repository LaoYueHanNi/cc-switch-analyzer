# DR: Antigravity 不可达时后台采集指数退避

Status: implemented

## Problem

Antigravity 的用量只存在于运行中的 language server（RPC 读取，不落盘），IDE 未打开是用户环境的常态。pending 会话存在时（`~/.gemini` 会话 mtime 晚于同步游标），后台采集每 5 秒一轮都会做进程枚举 + 端口探测并失败，同时每轮刷一条 WARN 日志——纯浪费且日志膨胀。

## Decision

后台采集改走 `scan_antigravity_with_backoff`：language server 不可达（未发现进程或 RPC 探测失败）时记录连续失败并指数退避（5s→10s→20s→40s→60s 封顶），退避窗口内直接返回空结果、跳过候选目录遍历与探测；任何一次成功立即重置退避。非端点类错误（如应用库故障）不退避，照常每轮重试。前端手动扫描命令 `scan_antigravity_now` 仍直调 `scan_antigravity`，不受退避限制——用户显式点击时立即探测。

## Alternatives considered

- **失败后固定等待（如常量 60s）** —— 实现更简，但 IDE 刚关闭后的第一次重试也被拉长，恢复不够平滑。
- **按「IDE 进程存在」做门控（先查进程再探测）** —— 失败路径的进程枚举本身就是要省的成本之一；且 IDE 在运行但 RPC 不可达的场景仍会反复探测。
- **禁用该源直到用户手动触发** —— 违背后台采集"IDE 打开后自动恢复"的既有预期。

## Consequences

- 换来：IDE 未运行时后台轮次几乎零成本，WARN 不再每 5 秒刷屏。
- 代价：IDE 打开后最多约 65 秒（60s 退避 + 一轮采集）才恢复增量采集；退避状态是模块级全局（跨数据源实例共享，与单实例应用语义一致）。
