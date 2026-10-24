# DR: 去重键抽象（DedupKey）与 request_id 贯通

Status: implemented

## Problem

跨源优先级去重（见 [跨源优先级去重](2026-10-24-cross-source-priority-dedup.md)）的配对条件硬编码为「四类 Token 一致 + 创建时刻/耗时对齐 + 一对一占用」，判重依据没有一等抽象。两个具体缺口：

1. **无 ID 判重途径**。用户期望「记录带相同请求 ID 即直接判重，ID 不一致才落入 Token 匹配」。核查数据链路后确认：跨源之间不存在可相等的请求 ID——Magpie 账本的 `rid` 是**上游厂商返回的请求 ID**（magpie 源码 `usage.go`："RequestID the id the vendor gave the call"，取自厂商响应头），DSH 会话日志不记录它（实测 rid 在 DSH 文件零命中）；应用库各源的 `request_id` 主键是**各自合成的**（DSH=`dsh:<消息id>`、Magpie=行内容哈希），体系互不相干。因此「跨源 ID 相等」在当前数据下恒不发生，直接把 ID 比较塞进现有配对逻辑是永远空转的死代码。
2. **实时行级判重靠七元组指纹，落库后被 UPDATE 的行会漏判**。60 秒重叠窗口（见 [实时流跨源去重重叠窗口](2026-10-24-realtime-stream-dedup-window.md)）使同一 DB 行被反复拉回，前端 `logKey` 用 dbType/providerId/session/时间/模型/Token/耗时 组合指纹去重；DSH 步进补耗时、Magpie 回填字段等落库后更新会让同一行的两次读取指纹不同，重复行穿透。

## Decision

把判重依据提升为**去重键**抽象，供给与判定解耦：

- `DedupKey::RequestId(String)`——强键：双方存在且相等 ⇒ 直接判定同一请求（跳过 Token 环节）；不相等或任一侧缺失 ⇒ 忽略本键，落入弱键判定。各源主键体系不同 ⇒ 跨源永不误配；同源重复行（同一 DB 行的重复拉取/导入）⇒ 被正确吸收。将来任两个源开始共享真实请求 ID（如 DSH 记录上游 rid），强键自动生效，匹配核心零改动。
- `DedupKey::TokenWindow { tokens, anchor, window_secs }`——弱键：原「四类 Token + 时间窗」规则原样收编。

匹配器 `dedup_by_keys(items, evidence_of)` 按键强度两阶段配对：tier1 强键建 HashMap 索引（O(1) 命中），tier2 强键先配；剩余走弱键的 anchor 排序 + 二分窗口。一对一占用与「宁可漏去重、不误杀」原则不变；tier 内部与未知层级不参与。旧的 `cross_source_priority_dedup` 泛型闭包签名被 `DedupEvidence`（tier + 键列表）取代，`cross_source_priority_dedup_records/streaming` 两个入口语义不变。

数据供给不动数据库：`session_request_logs.request_id` 主键本就存在。`StreamingRecord` 元组尾部追加第 12 列（request_id，空串=无），既有列位语义不变；`RawRecord` 增加 `request_id` 字段；全部 12 个数据源的 `get_filtered_records` 与 `get_recent_request_logs_raw` SELECT 带出该列。外部无此键的源（Cursor CSV、OpenCode 等）填空串，自动走纯弱键路径。`RealtimeRequestLog` 增加 `requestId` 返回前端，`logKey` 优先 `id|dbType|requestId`，为空退回七元组指纹——落库后 UPDATE 的行因主键稳定而正确判重。

**吸收清单下发**：实时链路的配对只保证「之后的轮次不再返回网关行」，但两源扫描轮错峰跨过前端 10 秒轮询边界时，网关行会先于端侧行被单独放出一轮并永久残留在前端列表（实测离线配对 234/234 全命中、页面残留率约 1/4/对）。`run_streaming_dedup` 因此返回 `(记录, absorbed)`——被吸收网关行中 request_id 非空者的清单；`query_realtime_logs` 以 `RealtimeLogsPage { logs, absorbed }` 返回，前端增量合并时按 requestId 移除已显示的残留行并清其指纹。配对决策仍只在后端一份，前端只执行移除指令；request_id 为空的被吸收行无法定位，不入清单（对应源本就少见于网关场景）。

## Alternatives considered

- **直接在现有配对函数里加 ID 相等分支**——当前数据下跨源 ID 恒不相等，分支永远不命中，是死代码；且键供给继续硬编码，下一个键类型（如上游 response id）又要改匹配核心。
- **仅改前端指纹**（把 requestId 拼进现有七元组）——只修实时页显示层，跨源配对仍无键抽象，RawRecord 统计链路（`fetch_deduped_records`）拿不到强键；且指纹串接顺序变化会掩盖「ID 是强判定、Token 是弱判定」的语义差别。
- **把 StreamingRecord 元组重构成 struct**——更整洁但触碰全部构造/解构点，与本次目标无关，留待后续；尾插列保持既有列位，代价是调用方依赖位置约定（已在类型注释中写明）。
- **残留行的前端定期全量对账**（每 60 秒全量拉取替换列表）——改动最小但清理延迟至多 60 秒，且全量替换打断「只增量追加」的渲染路径；吸收清单在下一个轮询周期（≤10 秒）内精确移除，无延迟。
- **残留行的前端补偿配对**（追加端侧行时在前端按弱键规则找网关行移除）——把配对逻辑复制到前端，必然口径漂移；吸收清单让后端唯一一份配对逻辑下发决策，前端只执行移除。

## Consequences

- 换来：判重键成为一等抽象，数据源按能力供给键（零个、一个或多个），匹配核心按强弱择优；同源行级判重由主键兜底，实时页在记录落库后被 UPDATE 的场景不再重复显示；统计链路（RawRecord）同步获得强键能力；实时页跨源残留行在下一个轮询周期内被精确清除。
- 代价：当前数据下跨源强键恒不命中（强键阶段对现有结果零影响，属前瞻性就位）；`StreamingRecord` 12 元组列位约定更依赖注释维护；各源 SELECT 多取一列（可忽略）；`run_streaming_dedup` 返回值变为元组、实时 IPC 契约改为 `RealtimeLogsPage`（调用方与测试同步调整）。
- 验证：`dedup.rs` 既有 30+ 用例全部保留守护弱键行为等价；新增强键用例（跨 tier 强键配对、ID 不等回退弱键、同 tier 不互吸、实时流第 12 列强键）与吸收清单用例（pipeline 层 absorbed 提取/空 ID 跳过、前端残留行移除/未知 ID 忽略）。
