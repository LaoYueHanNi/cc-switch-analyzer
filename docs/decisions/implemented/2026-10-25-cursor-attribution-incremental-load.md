# DR: Cursor 归因日志增量解析，消除每轮全量重读

Status: implemented

## Problem

本机精准归因每轮（`today_query` 的 5 秒门控 `refresh_cursor`，TrafficMonitor / 前端轮询驱动）调用 `cursor_local_hook::load_local_events()` 把 `~/.cursor/local-usage/requests.jsonl` 从头到尾逐行 JSON 解析。该日志由 hook 纯追加，99% 是从未变化的历史；实测 8.3MB / 3947 行的文件每 5–6 秒全量重解析一次，是应用静置时周期性 CPU burst（单核 85–119%、持续约 2 秒）的直接来源，随日志增长线性恶化。

## Decision

`load_local_events` 改为模块级增量缓存（`static LOCAL_EVENTS_CACHE`，按文件路径区分）：记录 `(mtime, size, 已消费字节偏移)` 与解析出的 events。文件未变（路径 + mtime + size 三者相同）直接复用缓存；变长则从上次偏移续读并只解析新增字节；截断或换文件时全量重扫。**半行保护**：hook 追加进行中末尾行可能不完整，只消费到最后一个换行符（含），残缺尾部留到下轮补全后再入库；JSONL 的 `\n` 不会出现在 UTF-8 多字节序列中间，按字节截断安全。行级解析抽为 `parse_event_line` 供全量与增量路径及测试共用。文件不可访问时返回空并作废缓存，与旧行为一致。缓存持锁覆盖读取与解析，避免并发调用（HTTP 服务线程与前端命令）重复追加。

## Alternatives considered

- **仅 mtime 未变时跳过** —— 实现最简，但 Cursor 活跃使用时 hook 持续追加、mtime 持续变化，每次变化仍要全量解析，burst 只能部分缓解。
- **增量 + 时间窗裁剪（丢弃 `attribution_filter_start` 之前的事件）** —— 内存更省，但当前日志几乎全部落在过滤窗内，裁剪收益接近零，徒增一条维护分支。
- **按行数或时间戳分片缓存** —— JSONL 无行索引，定位分片仍需顺序扫描，复杂度不成比例。

## Consequences

- 换来：静置 CPU burst 的主源消失；解析成本从 O(全部历史) 降为 O(本轮新增)；日志增长不再拖慢归因刷新。
- 代价：模块内新增可变全局状态（路径键控 + Mutex，测试用独立 tempdir 路径互不污染）；原地重写同长度文件的极端场景未被 `(mtime, size)` 双戳覆盖（hook 为纯追加，实际不会发生）；首次加载仍为一次全量解析。
