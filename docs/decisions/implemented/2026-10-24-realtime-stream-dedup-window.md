# DR: 实时流跨源去重重叠窗口与模型名归一化

Status: implemented

## Problem

两个实时页问题：

1. **模型名未归一**。ByModel 与趋势已按定价表归一化（见 [模型维度展示与算费按定价表归一化](2026-10-24-query-time-model-normalization.md)），但实时请求列表仍显示原始路由名（DSH 的 `group/glm-5.3-flashx`），与主页面口径分裂；费用因前缀兜底已能算对，唯独展示名不一致。

2. **跨源去重偶发失效**。实时列表同一请求出现 DSH 与 Magpie 两行（四类 Token 完全一致）。实测配对条件本身完美满足：Magpie 记录请求开始时刻（`created_at=…752`，latency 3265ms → anchor=`…755`），DSH 记录完成时刻（`created_at=…755`），anchor 与端侧完成时刻相等、Token 相等——静态配对无缺陷。根因在**增量轮次错过**：实时日志按 `stream_records(since)` 增量拉取，DSH 行（会话扫描）先入库、游标推进到 `…755`；Magpie 行（账本扫描）晚几秒入库后，增量游标永远拉不回 `created_at=…752` 的它，两条记录从不同时出现在同一配对窗口，跨源优先级去重系统性失配。「偶发」是因为仅当两源恰在同轮被拉到或页面首屏全量拉取时配对才成立。主页面统计查询走全量 `fetch_deduped_records`，不受影响；增量缓存更新（`refresh_database` 的 `run_streaming_dedup(since)`）同样存在此问题。

## Decision

`run_streaming_dedup` 的增量游标回退重叠窗口：`Some(since)` 时回退 60 秒（`stream_dedup_overlap_since`，下限 0），`None`（首屏全量）不变。重叠窗口内每轮都能看到同一请求的两条记录，跨源优先级配对在完整窗口上进行且幂等；重复拉回的记录由调用方既有指纹去重兜底——实时列表前端 `logKey`（含 dbType/providerId/sourceSessionId/时间/模型/Token/耗时）不重复追加，请求缓存 `RequestCache::merge` 按 RequestFingerprint 不重复累加。60 秒窗口覆盖后台采集 5 秒轮次的错峰与实测 -13~+1 秒的跨源时间差。

同时 `query_realtime_logs` 在构造 `RealtimeRequestLog` 前按 `resolve_model_id` 归一化模型名，展示与算费统一用 canonical id（未定价名保留原始名），与 ByModel 口径一致；归一化名进入前端指纹，同一记录跨轮结果稳定，不影响去重。

## Alternatives considered

- **前端跨轮配对池**（新 tier1 到达时回溯丢弃历史 tier2 行）—— 配对逻辑在后端已有一份，前端再实现一份必然口径漂移，且首屏全量与增量的边界处理复杂；重叠窗口让后端唯一一份配对逻辑自然工作。
- **持久化跨源豁免名单**（Magpie 入库时记录已被端侧吸收）—— 需要跨源状态写入与清理时机设计，账本替换/截断后状态失真；重叠窗口无状态、幂等。
- **实时页改为每轮全量拉取** —— 数据量随使用时长无界增长，违背增量游标设计；重叠窗口的重复量上界固定（60 秒窗口内的记录数）。

## Consequences

- 换来：实时列表同一请求单行显示（端侧优先），模型名与主页面一致；增量缓存更新同样修复跨源配对；改动仅一处游标回退 + 一处名字归一，无新状态。
- 代价：每轮多流至多 60 秒窗口的记录（量级为个位数到数十条，可忽略）；正确性依赖调用方指纹去重完整覆盖重复记录（新增 `run_streaming_dedup` 消费方时必须配套）；重叠窗口内的记录在配对被吸收前后可能短暂闪现（下一轮即被指纹吞掉）。
- 调试与验证：`stream_dedup_overlap_since`（None 不变、下限 0、回退 60s）由 `pipeline.rs` 单元测试覆盖；跨源配对与 anchor 对齐逻辑不变，沿用 `dedup.rs` 既有测试。
