# DR: 查询侧跨源优先级去重（端侧直录优先于统一网关）

Status: implemented

## Problem

同一真实请求会被两类数据源同时落库：端侧直录源（如 DSH 会话扫描）与统一网关转发账本（如 Magpie `usage.jsonl`、AIProxy、CCS）。DSH 的请求经 magpie 网关转发后，`source='DSH'` 与 `source='Magpie'` 各入一条库，主页面汇总、图表、实时请求列表与今日查询（TrafficMonitor/菜单栏）全部双计。

实测（2026-10-24）：Magpie 全部 60 条记录与 DSH 记录四类 Token 逐项一致，但时间差随请求时长漂移（0~60s 以上）——网关 `t` 是请求开始时刻，端侧 `created_at` 是响应完成落盘时刻；跨源之间模型名（`GLM-5.3-Flash` / `zcode/GLM-5.3-Flash` / `auto/GLM-5.3-Flash` / `glm-5.3-flash`）、session id 均不同，既有指纹去重（session_id + model + tokens）无法命中，且两源的入库 `request_id`（端侧内部消息 uuid、网关内容哈希）与网关 `rid`/`response_id` 互无交集，入库后无法按请求 id 关联。

## Decision

落库保持各源完整原始账本不动；在查询侧中心管道做跨源优先级去重，展示与算费统一按去重后口径。

- `services/dedup.rs` 新增 `cross_source_priority_dedup`（泛型核心）及 `cross_source_priority_dedup_records` / `cross_source_priority_dedup_streaming` 封装；层级清单 `CROSS_SOURCE_TIER1`（DSH、Cursor、OpenCode、ZCode、MiniMax、Proma、Antigravity、Kimi、PI）与 `CROSS_SOURCE_TIER2`（AIProxy、Magpie、CCS），`db_type` 为空时按 `provider_id` 兜底，其余层级不参与。
- 接入两处：`pipeline::fetch_deduped_records_report`（主查询、预计算与今日查询共用的中心管道）与 `pipeline::run_streaming_dedup`（实时请求列表，经 provider→数据源映射判定层级）。今日查询无需单独改动，自动继承。
- 配对规则（宁可漏去重、不误杀）：四类 Token 完全一致（model/session 不参与）；tier 2 记录带耗时（latency>0）时以「创建时刻 + 耗时」对齐 tier 1 完成时刻、窗口 ±10s，无耗时时创建时刻直比、窗口 ±30s；一对一占用，一条 tier 1 至多吸收一条 tier 2。同层之间不互配（CCS 与 AIProxy 的 codex 双记仍由既有指纹机制处理）。
- 真实数据验证：一对一贪心配对下 Magpie 60/60 唯一匹配、0 歧义、0 漏（仅响应生成中的瞬时窗口例外，落盘后即可配对）。

## Alternatives considered

- **入库时跳过（Magpie 扫描按 `agent='dsh'` 过滤，或入库前查端侧已有记录）**：需要改扫描器与增量游标语义，且 `agent` 是 magpie 私有字段约定；同时改变落库口径，历史清理与重扫流程复杂。落库即事实，查询口径分层更符合「账本完整、展示去重」的既有架构。
- **请求 id 关联（解析并存储上游 responseId，schema 迁移）**：DSH 会话确实在 `replayState.response.responseId` 保留了上游响应 id，与网关 `response_id` 100% 可关联，但需要双侧扫描器改造、表结构迁移与历史回填，复杂度高；用户明确否决。
- **读取侧固定 ±10s 时间窗（网关创建时刻直比）**：真实数据仅 54% 命中——时间差等于请求耗时，长请求全部漏配。改为「创建时刻 + 耗时」对齐后 96%（±5s）/98%（±10s），窗口内歧义与误杀均为 0。
- **分钟级趋势（`get_minute_level_token_trend`）桶级去重**：桶内无记录级依据；且扫描型源（含 Magpie）该接口均返回空，当前无重复场景，不做。

## Consequences

- 得：主查询、预计算、今日查询、实时请求列表四条路径口径自动一致；新增数据源只需归类层级清单；历史数据无需清理或重扫；规则参数（窗口、层级）集中在 `dedup.rs`，有 9 个单元测试覆盖配对、一对一、层级互斥与兜底。
- 代价：跨层两个真实请求四类 Token 恰好一致且落入窗口时会被误合并一条（概率极低，规则取向为宁漏勿杀）；查询侧多一次 tier 1 排序配对（O(N log N)，无网关记录时走快速路径零开销）；实时流从单遍流式指纹去重变为「收集后指纹 + 跨源」两段（数据量受 `SESSION_TOP_N` 约束）；CCS/AIProxy/OpenCode 之间的分钟趋势重复维持现状；±10s 窗口的命中率依赖网关源 `latency` 字段质量，缺失时退化 ±30s 直比。
