# DR: OpenCode 过滤查询按源库内容戳短路，跳过未变化的全表扫描

Status: implemented

## Problem

OpenCode 源库（`~/.local/share/opencode/opencode.db`，实测 198MB）是外部只读库，铁律禁止加索引；其用量存于 message 表的 `data` JSON 列，过滤查询的 WHERE 含 `json_extract(data, '$.role')` 行级解析与 `(time_created/1000)` 表达式包裹，索引不可达，必然全表扫描（实测 27ms/次）。TrafficMonitor 插件每秒轮询 `/api/today`、应用前端页面打开时亦轮询，每次都对 OpenCode 执行该扫描——当日 0 行用量也要读并解析全部 1013 行大 JSON，构成静置时每秒 CPU 小峰与约 11MB/s 的恒定磁盘读取。

## Decision

`OpenCodeDbService` 增加 `filtered_cache`：以主库与 `-wal` 文件的 `(mtime, size)` 四元组内容戳（`ContentStamp`，stat 两个文件微秒级）判定源库未变，且查询参数与缓存一致时直接复用上次 `get_filtered_raw_records` 结果。参数键为 `build_where_clause` 实际消费的 `(from_epoch, provider_id, model_id)`（ccs 过滤对 OpenCode 不生效，不入键）；`to_epoch` 按"内容未变时数据集合固定"的单调性处理：本次 `to ≥ 缓存 to` 才复用，`to` 缩小的历史窄区间查询强制重查。写回时同内容下保留上界更大的缓存，避免窄区间查询挤掉高频 today 轮询的宽缓存。`close()` 清空缓存防止换库携带旧结果；查询失败不动缓存（内容戳已不匹配，旧缓存不会误用）。

## Alternatives considered

- **把 OpenCode 用量增量导入应用库（ZCode 模式）** —— 彻底根治（查询走自家索引），但需导入器、增量状态、读取切换与 OpenCode 数据结构演进维护，工程量大；作为后续版本的候选，先以短路拿主要收益。
- **仅改写 SQL 去掉表达式包裹** —— message 表只有 `(session_id, time_created, id)` 复合索引且前导列不是时间，去包裹后仍无法索引可达，白改。
- **在 today_query/pipeline 层做通用 content_mtime 短路** —— `DataSource::content_mtime` 契约存在但仅 Cursor 实现；通用化要定义全部 13 源的"内容未变"语义，影响面大，先按需落在最贵的 OpenCode。
- **`/api/today` 加轮询微缓存** —— 违背"按请求计算，不缓存 30 秒结果"的既有决策精神，且短路后每次全链查询剩余成本已降至毫秒级。

## Consequences

- 换来：源库静置时每秒轮询的 OpenCode 全表扫描与对应 IO 消失；stat 成本可忽略。
- 代价：`OpenCodeDbService` 持有可变缓存状态（`Mutex<Option<…>>`，today 与图表查询并发安全）；OpenCode 活跃使用时（库每次变化）仍要付一次全表扫描，缓存只在两次变化之间生效；`(mtime, size)` 双戳不覆盖原地重写同长度文件的极端场景（SQLite 正常写入不发生）。
