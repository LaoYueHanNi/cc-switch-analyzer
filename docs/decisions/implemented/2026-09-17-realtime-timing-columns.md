# DR: 实时页拆分「首字」「耗时」「输出/总 速度」三列与全数据源指标适配

Status: implemented

## Problem

此前在 [2026-09-17-realtime-source-filter.md](./2026-09-17-realtime-source-filter.md) 早期方案中曾尝试将原「延迟」列（`latencyMs`）直接更名为「首字」，导致了严重的指标混淆：
1. **名不副实**：将请求全长耗时（`latencyMs`）标为「首字」，违背了流式首包时间（TTFT）的本义。
2. **能力被掩盖**：具备真实首包时间的源（如 Antigravity、CCS、ZCode）无法展示其实际首字延迟；无 TTFT 的源则被误以为首字极慢。
3. **输出速度失真与歧义**：单轨输出速度无法兼顾有首字流式源的纯吐字速率与整包/无首字源的端到端总速度，按源分治或单值展示均存在认知偏差。
4. **底层支持不均衡**：底层 10 个数据源（Antigravity、CCS、ZCode、OpenCode、AIProxy、Proma、DSH、Kimi、MiniMax、Cursor）在首字与耗时上的底层支持参差不齐，需逐源排查底牌与制定接入规范。

## Decision

实时页确立了表头权责清晰的时间与速率三列度量体系，并逐源落实接入：

1. **「首字」**：展示第一个 token 的到达时间（`timeToFirstToken`），由 `formatTtft` 格式化；未采集或无此字段的数据源显示 `-`。
2. **「耗时」**：展示整个请求的完整耗时（`latencyMs`），由 `formatLatency` 格式化（>= 1000ms 显示为 Xs，否则显示 Xms；`<= 0` 与非数值一律显示 `-`）。
3. **「输出/总 速度」**：采用 **`A/B tok/s`** 双轨直显体系：
   - **A（纯吐字速度，青色加粗）**：扣除首字耗时的流式速率 $\text{tokens} / ((\text{耗时} - \text{首字}) / 1000)$。只要满足 $\text{latencyMs} > \text{ttft} > 0$ 即如实按物理时间差计算，无人工截断门限；源头无首字或 $\text{耗时} \le \text{首字}$ 时安全显示为 `-`；
   - **B（端到端总速度，橙色）**：基于全长耗时的综合吞吐速率 $\text{tokens} / (\text{耗时} / 1000)$；
   - **悬停 Tooltip**：直显两项速度具体数值（纯吐字/端到端），省略冗长推导说明。

本记录含两类内容，**必须分开读**：「全数据源源头支持评估」是对各源底层存储的**实勘评估**（源头是否提供、不适用者如何降级），「各源接入现状」才是**已落地的实现**。评估结论为 ✅ 不等于该源已接入，反之亦然。

### 全数据源源头支持评估

经过对全部 10 个数据源底层原生存储（JSONL / SQLite / CSV）的逐一实勘，源头支持与降级策略如下：

| 数据源 | 数据源类型 | 首字（TTFT）源头支持 | 耗时（latency）源头支持 | 双轨形态 (A/B tok/s) | 评估结论与降级策略 |
|---|---|:---:|:---:|:---:|---|
| **Antigravity** | 扫描型 (JSONL) | ✅ 原生 `first_token_latency` | ✅ 原生 `latency` | `A / B tok/s` | 源头两者完备，可直接如实展示 |
| **CCS** | 直连型 (SQLite) | ✅ 原生 `first_token_ms`（备份库实测有该列） | ✅ 原生 `latency_ms` | `A / B tok/s` | 需动态探测列：旧版 cc-switch 无 `first_token_ms`，缺列时安全回退 0 |
| **ZCode** | 扫描型 (SQLite) | ✅ 原生 `time_to_first_token_ms` | ✅ 原生 `duration_ms` | `A / B tok/s` | 源表 `model_usage` 有该列（本机实测），扫描时提取即可 |
| **OpenCode** | 直连型 (SQLite) | ✅ 附属 `part.data.time.start`（实测 355/367 可算） | ✅ `time.completed - created` | `A / B tok/s` | 首字不在主表，须关联 `part` 表；无正文 part 时显示 `-` |
| **AIProxy** | 直连型 (SQLite) | ❌ `token_stats` 无首字字段 | ✅ `duration_ms` | `- / B tok/s` | 首字不可得，A 轨自然占位 `-`；B 轨可用 |
| **Proma** | 扫描型 (JSONL) | ❌ 源头无首字字段 | ⚠️ 旧格式有 `durationMs`；SDK 格式无单次耗时（`result` 为整轮汇总行） | `- / B tok/s`（旧格式）或 `-`（SDK 格式） | 旧格式可取耗时；SDK 格式须保持 `-`，不可误将轮汇总当作单次 |
| **DSH** | 扫描/插件双轨 | 会话扫描：✅ 会话事件流含毫秒 `time`（实测 470/474 step 可配对）；插件数据：✅ 原生 `firstTokenLatencyMs` | 会话扫描：✅ 同上推得；插件数据：✅ 原生 `latencyMs` | `A / B tok/s` | 两条路径源头均可得，取得方式见下方实勘记录 |
| **Kimi** | 扫描型 (JSONL) | ❌ `wire.jsonl` 仅记账无时间 | ❌ 无耗时字段 | `-` | 不适用，源头纯记账，无任何时间信息 |
| **MiniMax** | 扫描型 (SQLite) | ✅ `thinking_duration_ms`（[见 2026-09-19 决策](./2026-09-19-minimax-sqlite-migration.md)） | ✅ `request_duration_ms`（同上） | `A / B tok/s` | 直读 `runtime-state.sqlite::local_runtime_message_rows` 表 |
| **Cursor** | 文件型 (CSV) | ❌ CSV 仅有账单字段无首字 | ❌ CSV 无耗时 | `-` | 不适用，仅为静态用量导入 |

### 实勘记录

> **实证（2026-09-18，CCS 行）**：本机 `~/.cc-switch/cc-switch.db` 为 0 字节空库，改查备份 `backups/db_backup_20260521_154643.db`：`proxy_request_logs` 共 25 列，含 `latency_ms`、`first_token_ms`、`duration_ms`，确认两列源头存在（该备份仅 1 行数据且 `first_token_ms` 为 0，实际非零率需以使用库为准）。后续对使用库 `E:\Documents\CC-Switch\cc-switch.db` 的统计见「各源接入现状」。

> **实勘修订（2026-09-18，OpenCode 行）**：原判「❌ 无首字」仅翻查了 `message` 表，未翻查附属的 `part` 表，属实勘遗漏。本机 `opencode.db` 实测：`part` 表 1729 行中 `reasoning` part 332/332、`text` part 135/227 带 `data.time.start`；以该 step 首个 `step-start` part 的 `time_created` 为请求起点，368 条 assistant 消息中 356 条可配对算出首字，TTFT 分布 min=1ms / p50=356ms / max=3171ms，无负值。

> **实勘修订（2026-09-18，DSH 行）**：原判「🔄 待外部插件」未区分 DSH 的两条接入路径。**会话扫描路径**（`dsh_use_plugin=0`，见 `dsh_scanner.rs`）读的就是 `~/.dsh/sessions/**/session.jsonl.zstd`，该文件事件流本身带毫秒 `time`：本机实测 20552 个事件中 7135 个带 `time`，474 个 `step/start` 里 470 个能同时配到 `assistant/chunk` 与 `assistant/message`（首个 chunk 时间 − `step/start` 时间 = 首字，`assistant/message` 时间 − `step/start` 时间 = 耗时，实测 3964ms / 9744ms）；同文件另有 `text-chunks`/`reasoning-chunks`/`tool-call-chunks` 带 `time0`+`dt[]` 增量数组可交叉校验（两路互差 0~112ms）。**插件数据路径**（`dsh_use_plugin=1`）原行格式仅 `{requestId,time,sessionId,model,usage}`（见 `dsh_plugin_scanner.rs`），无任何时间字段，需插件先扩展；插件（dsh-token-usage）随后已写入 `latencyMs` / `firstTokenLatencyMs`（本机实例 8480 行中 7745 行带该二字段）。

### 各源接入现状

下表为本机 `pricing.db::session_request_logs` 的实测统计（扫描型源；直连型源按其源库单独核查）：

| 数据源 | 库内记录 | 耗时 > 0 | 首字 > 0 |
|---|---:|---:|---:|
| DSH | 8325 | 138 | 138 |
| ZCode | 2913 | 2913（100%） | 15（0.5%） |
| Antigravity | 252 | 252（100%） | 210（83.3%） |
| MiniMax | 6 | 0 | 0 |
| Proma | 6 | 0 | 0 |

两侧均无「首字 > 耗时」倒挂。

> **时序字段不回填（实测结论）**：`insert_session_log_on_conn` 的 UPSERT 仅在四维 token 总和变大时更新整行，扫描型的增量游标又只前进不回扫。当源头对**已存在的行**回填时间字段（ZCode 的 `time_to_first_token_ms`、DSH 插件的 `latencyMs`/`firstTokenLatencyMs`），analyzer 库里的旧行不会被刷新，仍显示 `-`。实测：ZCode 源库 1802 条可扫描行中 1502 条（83.3%）带首字，analyzer 库同批历史行首字恒为 0 —— 抽样四条 `latency` 与源库分毫不差（14778/22514/2932/7159），而 `first_token_latency` 全为 0。上表中 DSH 的 138 条即本次改造后新入库的行，ZCode 的 15 条同理。**已决定不修**：实时页只展示近期记录，回填后的新记录本就正常，收益仅落在不被展示的历史数据上（理由见 Alternatives）。

- **DSH**：两条路径均已接入。会话扫描新增 `DshParseState` 维护跨行时序（`step/start` 起点、首个 `assistant/chunk` 首字、`assistant/message` 终点，按 turn/step 校验，失配或起点缺失时回退 0）；插件路径直读 `latencyMs` / `firstTokenLatencyMs`；统一经 `ParsedRow.first_token_latency` 落库，`scan_file_incremental` 改为对旧行仍调用解析器以重建跨行上下文、只让新行入库。按源头数据解析实测：会话扫描 8191 条（耗时 100%、首字 82.9%），插件数据 8200 条（均 94.8%）。
- **OpenCode**：已接入。实时查询抽出公共 `REALTIME_COLUMNS` / `REALTIME_FILTER`，首字取「首个带 `data.time.start` 的 part 时间 − `message.time.created`」，无正文 part 回退 0，负偏移 clamp。实测 367 条（耗时 100%、首字 96.7%），样例 `latency=3135ms / ttft=2327ms`。
- **CCS**：读取侧已就绪（动态探测 `first_token_ms` 列，缺列回退 0）。本机真实库（62534 行）按 analyzer 的实时 SQL 端到端核查：当前过滤配置（`ccs_filter_session_apps=["opencode"]`）下可见 56223 条，其中 44712 条有耗时（79.5%）、38797 条有首字（69.0%）。无时间数据的是 `data_source='session_log'` 的会话同步记录（11065 条，cc-switch 对其不写 `latency_ms`/`first_token_ms`），按设计显示 `-`。
- **AIProxy**：耗时读取侧已就绪（`duration_ms`，本机 `~/.ai-agent-tools/data/access_log.db` 实测 2107/2130 > 0）；源头 `token_stats` 16 列中无首字字段，A 轨自然占位 `-`。
- **ZCode**：读取侧与入库链路已就绪。实时查询直读 `session_request_logs`，抽样核对 `latency` 与源库完全一致；首字受上述不回填影响，历史行为 `-`，改造后新入库的记录正常显示（实测 15 条）。
- **Antigravity**：源头原生两列均入库，实测 252 条中 210 条带首字。
- **Proma**：旧格式 `durationMs` 读取侧已就绪；SDK 格式整轮汇总行保持 `-`。库内样本仅 6 条且均为无耗时形态。
- **MiniMax**：已于 2026-09-19 升级为直读 `runtime-state.sqlite::local_runtime_message_rows`，支持毫秒级 `request_duration_ms` 与 `thinking_duration_ms`（详见 [2026-09-19 决策](./2026-09-19-minimax-sqlite-migration.md)）。
- **Kimi / Cursor**：源头缺时间字段，三列如实显示 `-`（评估表已列明），无读取侧改动。

## Alternatives considered

- **维持单列「首字」并展示整段耗时** —— 混淆首包延迟与总耗时，造成严重数据失真，坚决废弃。
- **单值按源策略切换速度计算** —— 容易引起用户对同一列数值定义不一致的困惑；双轨 `A/B tok/s` 一目了然且兼顾纯流速与端到端吞吐。
- **保留 1000ms 纯流式截断门限** —— 短文本或快模型（如 Gemini Flash 在首字后 800ms 吐完）会被误判为整包爆发而退化为 `-`；移除人工门限、以实际数学差值计算更客观真实。
- **OpenCode 首字改用 `step-start` part 的 `time_created` 为起点** —— 能得到纯模型首包时间（实测 p50=356ms，不含排队），但与同表既有耗时列（起点为 `message.time.created`）不同源，会使 A 轨「纯吐字速度」的分子分母跨两个起点而失去可比性，且与其余数据源「用户等待」的语义不一致。故统一沿用 `message.time.created` 起点，代价是首字含约 2 秒排队/准备时间。
- **DSH 会话扫描改读 `text-chunks`/`reasoning-chunks` 的 `time0`+`dt[]` 增量数组** —— 与首个 `assistant/chunk` 的时间戳互为校验（两路互差 0~112ms），精度相当但需解析数组增量，首个 chunk 路径更直接，故取后者。
- **把「源头支持评估」与「接入实现」写进同一张表** —— 会使 ✅ 被误读为「已实现」（本记录曾因此被误迁为 implemented 状态）。故拆成「评估表 + 接入现状」两张清单，各自独立更新。
- **修掉扫描型源的时序回填缺口（回扫时间窗 + 放宽 UPSERT）** —— 可让 ZCode 历史首字覆盖率从 0.5% 提到源头侧的 83.3%，但要动 `insert_session_log_on_conn` 这条所有扫描源共用的 UPSERT，而 Proma 的分片快照「取最大」语义正依赖现有 WHERE 条件，放宽会波及无关源；收益又只落在不被展示的历史行上（实时页只看近期，回填后的新记录本就正常）。故不修。

## Consequences

- 换来：三列度量权责清晰，「首字」不再与总耗时混淆；有原生首字的源（Antigravity / CCS / ZCode / OpenCode / DSH）能如实展示首包延迟与纯吐字速率，无首字的源用 `-` 明确降级而非伪造数值；全 10 个数据源的源头底牌一次性查清并落档，后续新增源可按同一张评估表接入。
- 换来：`formatLatency` 与 `formatTtft` 对 `<= 0` 与非数值统一返回 `-`，同一行的「首字」「耗时」空值语义一致（此前 `0` 会渲染成 `0ms` 而首字渲染成 `-`）。
- 代价：三列形态随源而异（`A/B`、`- / B`、`-`），用户需理解同一列在不同源上语义不同；A 轨恒依赖源头首字，$\text{耗时} \le \text{首字}$ 时为 `-`。
- 代价：源头有列不等于有值。CCS 真实库实测覆盖率 79.5%（耗时）/ 69.0%（首字），缺口来自 cc-switch 自身对 `session_log` 类记录不写时间列；若该比例长期不变，首字列在这些源上会保持大面积 `-`。
- 代价：时序字段对**已入库的历史行**不回填（见上方实测）。DSH 插件侧的时序字段还是请求结束后回填的，老记录与 `kind:"failure"` 失败行不带该二字段（实测约 5%），此类记录保持 `-`，不回退成耗时 0 参与速度计算。
- 代价：各源首字口径由其源头定义 —— OpenCode 取 `message.time.created`（含约 2 秒排队/准备），DSH 取 step 起点，CCS / Antigravity / ZCode 取源头原生字段。跨源比较首字时需知悉此差异。
