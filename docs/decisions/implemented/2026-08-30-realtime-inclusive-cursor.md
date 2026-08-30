# DR: 实时增量游标采用 >= 闭式语义，重复由前端指纹去重兜底

Status: implemented

## Problem

实时页 10s 轮询 `query_realtime_logs` 用秒级游标做增量拉取。ZCode 的 `started_at` 为毫秒，`stream_records` 返回时 `/1000` 截断成秒，而过滤却用 `started_at > since*1000` 毫秒精确比较：已见最新记录的毫秒尾数使其每轮轮询都重复满足条件，前端又无去重直接拼接，实时页每 10s 叠加一条一模一样的记录、顶部总费用/总Token 随之虚高。同时其余各源的秒级游标用严格大于 `>`，同一秒内后到的记录会被永久跳过（低概率丢记录）。

## Decision

全源统一增量游标语义为「返回 created_at >= since，且过滤精度与返回 created_at 的精度一致」：

- 各数据源 `stream_records` / `get_recent_request_logs_raw` 的 since 过滤由 `>` 改为 `>=`；毫秒存库的源（ZCode）在 SQL 内同样截断到秒：`(started_at / 1000) >= ?`。
- `>=` 会重复返回游标秒内已见记录，由前端 `useRealtimePolling` 的持久指纹集（`dbType|providerId|sessionId|createdAt|model|tokens|latencyMs`）去重兜底；`refreshNow` 同步重置指纹集。
- 契约文档化在 `DataSource::stream_records` trait 注释（`src-tauri/src/services/data_source.rs`），未来新数据源接入按此实现。

## Alternatives considered

- **ZCode 返回毫秒精度 createdAt、前端用毫秒游标** —— 语义最精确，但 `RealtimeRequestLog.created_at` 是全源统一的秒级字段，毫秒值混入会破坏全局倒序排序与展示，需要为游标单开字段并改动全部 9 个源的返回签名，改动面与风险不成比例。
- **仅 ZCode 在 SQL 内截断后保持严格大于 `(started_at/1000) > ?`** —— 一行改动即可消除重复，但 ZCode 会从"重复"变成"丢同秒记录"（ZCode 高频并行场景下同秒多条请求概率不低），且其余源的同秒丢记录局限依旧存在。
- **per-source 游标（前端按 dbType 维护多游标，`since` 参数改为按源分发）** —— 需要变更 `query_realtime_logs` 的前后端协议，仅为单一精度问题不值。

## Consequences

- 换来：所有数据源实时页既不重复也不丢同秒记录；游标语义全源统一并文档化，新源接入有明确契约可循。
- 代价：每轮轮询多传输游标秒内的少量已见记录（量级可忽略）；前端 `seen` 指纹集成为正确性的必要组成部分，重构 `useRealtimePolling` 时不可移除；理论边界——同一秒内 token 与延迟完全相同的两条真实请求会被误判为重复（概率趋近零，且仅影响实时页显示，不影响统计页计数）。
