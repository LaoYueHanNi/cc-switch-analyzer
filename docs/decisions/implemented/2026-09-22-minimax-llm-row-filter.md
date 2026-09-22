# DR: MiniMax 只入库带请求耗时的 LLM 响应

Status: implemented

## Problem

[MiniMax 直读 SQLite](2026-09-19-minimax-sqlite-migration.md) 之后，扫描器把 `local_runtime_message_rows` 里 token 非零的 assistant 行都当成一次请求。MiniMax Code 写 assistant 行时会把会话累计的 `context_usage` 填进 `usage.input_tokens` / `usage.output_tokens`，greeting、工具中转、纯文本回复因此也带上一组假 token。这些行没有 `usage.request_duration_ms`（入库后 `latency = 0`），统计把累计上下文当成单次消耗。

## Decision

`parse_minimax_sqlite_row` 只接受 `usage.request_duration_ms > 0` 的行，不再用 token 是否非零判断有效性。`latency` 仍映射该字段。

`app_db::migrate_v16` 删除已入库的 `source = 'MiniMax' AND latency = 0` 记录，并清掉 MiniMax 的 `session_log_sync` 游标，让新条件重新扫描。其他数据源不动。

## Alternatives considered

- **继续按 token 非零过滤** —— 这是 v15 直读后的做法。累计 `context_usage` 让非 LLM 行的 token 也非零，挡不住脏数据。

## Consequences

- 换来：MiniMax 统计只含真实 LLM 响应；已误入的零耗时记录在升到 v16 时清掉并重扫。
- 代价：没有 `request_duration_ms` 的真实调用也不会入库。升级时 MiniMax 缓存清空一次再导入，其他数据源不受影响。
