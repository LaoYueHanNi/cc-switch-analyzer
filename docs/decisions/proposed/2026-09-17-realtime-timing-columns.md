# DR: 实时页拆分「首字」「耗时」「输出/总 速度」三列与全数据源指标适配

Status: proposed

## Problem

此前在 [2026-09-17-realtime-source-filter.md](../implemented/2026-09-17-realtime-source-filter.md) 早期方案中曾尝试将原「延迟」列（`latencyMs`）直接更名为「首字」，导致了严重的指标混淆：
1. **名不副实**：将请求全长耗时（`latencyMs`）标为「首字」，违背了流式首包时间（TTFT）的本义。
2. **能力被掩盖**：具备真实首包时间的源（如 Antigravity、CCS、ZCode）无法展示其实际首字延迟；无 TTFT 的源则被误以为首字极慢。
3. **输出速度失真与歧义**：单轨输出速度无法兼顾有首字流式源的纯吐字速率与整包/无首字源的端到端总速度，按源分治或单值展示均存在认知偏差。
4. **底层支持不均衡**：底层 10 个数据源（Antigravity、CCS、ZCode、OpenCode、AIProxy、Proma、DSH、Kimi、MiniMax、Cursor）在首字与耗时上的底层支持参差不齐，需要逐源排查底牌与制定接入规范。

## Proposal

彻底确立实时页表头权责清晰的时间与速率三列度量体系，分类推进各数据源底层支持：

1. **「首字」**：展示第一个 token 的到达时间（`timeToFirstToken`），由 `formatTtft` 格式化；未采集或无此字段的数据源显示 `-`。
2. **「耗时」**：展示整个请求的完整耗时（`latencyMs`），由 `formatLatency` 格式化（>= 1000ms 显示为 Xs，否则显示 Xms）。
3. **「输出/总 速度」**：采用 **`A/B tok/s`** 双轨直显体系：
   - **A（纯吐字速度，青色加粗）**：扣除首字耗时的流式速率 $\text{tokens} / ((\text{耗时} - \text{首字}) / 1000)$。只要满足 $\text{latencyMs} > \text{ttft} > 0$ 即如实按物理时间差计算，无人工截断门限；源头无首字或 $\text{耗时} \le \text{首字}$ 时安全显示为 `-`；
   - **B（端到端总速度，橙色）**：基于全长耗时的综合吞吐速率 $\text{tokens} / (\text{耗时} / 1000)$；
   - **悬停 Tooltip**：直显两项速度具体数值（纯吐字/端到端），省略冗长推导说明。

### 全数据源排查现状与接入清单

经过对全部 10 个数据源底层原生存储（JSONL / SQLite / CSV）的逐一实勘，三列指标底牌与接入方案如下：

| 数据源 | 数据源类型 | 首字（TTFT）源头支持 | 耗时（latency）源头支持 | 双轨展示形态 (A/B tok/s) | 实施结论与现状 |
|---|---|:---:|:---:|:---:|---|
| **Antigravity** | 扫描型 (JSONL) | ✅ 原生 `first_token_latency` | ✅ 原生 `latency` | `A / B tok/s` | ✅ 原生首字与耗时完备，如实展示纯吐字速度与端到端总速度 |
| **CCS** | 直连型 (SQLite) | ✅ 原生 `first_token_ms` (2.1万条实测) | ✅ 原生 `latency_ms` (100%) | `A / B tok/s` | ✅ 动态探测列容错，提取首字写入元组第 9 位，旧库安全回退 0 |
| **ZCode** | 扫描型 (SQLite) | ✅ 原生 `time_to_first_token_ms` | ✅ 原生 `duration_ms` (100%) | `A / B tok/s` | ✅ 扫描器提取写入 `session_logs.first_token_latency`，单测全绿 |
| **OpenCode** | 直连型 (SQLite) | ✅ 关联 `part.data.time.start`（368 条 assistant 消息实测 356 条可配对，p50=356ms） | ✅ `time.completed - created` (100%) | `A / B tok/s` | 🔄 首字源头可得（首个正文 part 的 `time.start` 减该 step 的 `step-start.time_created`），`opencode_db.rs` 现硬编码 0，待接入 |
| **AIProxy** | 直连型 (SQLite) | ❌ `token_stats` 无首字字段 | ✅ `duration_ms` (100%) | `- / B tok/s` | ✅ 耗时完备，A 自然占位 `-`，B 端到端总速度 |
| **Proma** | 扫描型 (JSONL) | ❌ 源头无首字字段 | ⚠️ 旧格式有 `durationMs`；SDK格式无单次耗时（`result`为整轮汇总行） | `- / B tok/s`（旧格式）或 `-`（SDK格式） | ➖ 维持现状（已支持旧格式耗时；SDK格式保持安全 `-`，不可误将轮汇总当作单次） |
| **DSH** | 扫描/插件双轨 | 会话扫描（缺省）：✅ 同一会话文件事件流（实测 470/474 step 可配对）；插件数据：❌ 行格式无首字字段 | 会话扫描（缺省）：✅ 同上推得；插件数据：❌ 待扩展 `latencyMs` | 会话扫描：`-`（现 `latency=0` 致 B 亦退化为 `-`），接入后 `A / B tok/s`；插件数据：升级后 `A / B tok/s` | 🔧 会话扫描路径源头可得、待接入（`dsh_scanner.rs` 现写死 `latency: 0`、传参 `first_token_latency: 0`）；🔄 插件路径待插件扩展 `firstTokenLatencyMs` / `latencyMs` |
| **Kimi** | 扫描型 (JSONL) | ❌ `wire.jsonl` 仅记账无时间 | ❌ 无耗时字段 | `-` | ➖ 不适用（源头纯记账，无任何时间信息） |
| **MiniMax** | 扫描型 (JSONL/SQLite) | ❌ 仅有结束时间戳 | ❌ 无耗时字段 | `-` | ➖ 不适用（源头仅记结束点，无交互持续时间） |
| **Cursor** | 文件型 (CSV) | ❌ CSV 仅有账单字段无首字 | ❌ CSV 无耗时 | `-` | ➖ 不适用（仅为静态用量导入） |

> **实勘修订（2026-09-18，OpenCode 行）**：原判「❌ 无首字」仅翻查了 `message` 表，未翻查附属的 `part` 表，属实勘遗漏。本机 `opencode.db` 实测：`part` 表 1729 行中 `reasoning` part 332/332、`text` part 135/227 带 `data.time.start`；以该 step 首个 `step-start` part 的 `time_created` 为请求起点，368 条 assistant 消息中 356 条可配对算出首字，TTFT 分布 min=1ms / p50=356ms / max=3171ms，无负值。首字源头可得，接入待独立立项。

> **实勘修订（2026-09-18，DSH 行）**：原判「🔄 待外部插件」未区分 DSH 的两条接入路径，须按路径分判。**会话扫描路径**（`dsh_use_plugin=0`，缺省，见 `dsh_scanner.rs:595`）读的就是 `~/.dsh/sessions/**/session.jsonl.zstd`，该文件事件流本身带毫秒 `time`：本机实测 20552 个事件中 7135 个带 `time`，474 个 `step/start` 里 470 个能同时配到 `assistant/chunk` 与 `assistant/message`（首个 chunk 时间 − `step/start` 时间 = 首字，`assistant/message` 时间 − `step/start` 时间 = 耗时，实测 3964ms / 9744ms）；同文件另有 `text-chunks`/`reasoning-chunks`/`tool-call-chunks` 带 `time0`+`dt[]` 增量数组可交叉校验（两路互差 0~112ms）。此路径与既有实现同文件、同解析器、同增量机制，属既有路径延伸而非新增耦合，故源头可得、待接入。**插件数据路径**（`dsh_use_plugin=1`）行格式仅 `{requestId,time,sessionId,model,usage}`（见 `dsh_plugin_scanner.rs:8-13`），无任何时间字段，确须等插件扩展 `firstTokenLatencyMs` / `latencyMs`。

## Alternatives considered

- **维持单列「首字」并展示整段耗时** —— 混淆首包延迟与总耗时，造成严重数据失真，坚决废弃。
- **单值按源策略切换速度计算** —— 容易引起用户对同一列数值定义不一致的困惑；双轨 `A/B tok/s` 一目了然且兼顾纯流速与端到端吞吐。
- **保留 1000ms 纯流式截断门限** —— 短文本或快模型（如 Gemini Flash 在首字后 800ms 吐完）会被误判为整包爆发而退化为 `-`；移除人工门限、以实际数学差值计算更客观真实。

## Acceptance criteria

- 前端实时表格支持展示首字、耗时与双轨速度 `A/B tok/s`。
- Antigravity、CCS、ZCode 原生首字与耗时提取接入完毕，旧库缺列安全降级。
- 待外部插件发布的渠道（如 DSH）在插件版本发布后独立接入。

## Risks

- DSH 依赖外部插件版本发布，本案先敲定协议格式，落地作为独立子任务跟进。
- 部分数据源（Kimi、MiniMax、Cursor）原生不具备时间信息，需明确保持 `-` 避免误导。