# DR: 实时页改为按数据源筛选

Status: implemented

## Problem

实时页工具栏原本按模型过滤，与主页 FilterBar 的数据源下拉不一致：当多个数据源（CCS、OpenCode、Antigravity、Cursor 等）混在一张表里时，用户无法按数据源快速收窄最近请求；同时实时页需要独立于全局筛选的即时切源能力。

## Decision

实时页工具栏复制主页数据源 CompactSelect（`filterStore.providerOptions`，placeholder「全部」），用独立的 `selectedSource` 响应式变量按数据行的 `dbType` 过滤。不绑定全局 `filterStore.providerId`，避免在实时页切换筛选意外改变模型/供应商等分析页面的查询状态。

> **修订说明**：此前随本决策草案附带的「表头延迟改名为首字」临时方案已被 [2026-09-17-realtime-timing-columns.md](../proposed/2026-09-17-realtime-timing-columns.md) 正式推翻与系统重构，实时页确立了「首字」「耗时」「输出/总 速度」独立度量体系。

## Alternatives considered

- **绑定 `filterStore.providerId` 与主页共用筛选状态** —— 实时页改源会联动修改模型/供应商页的查询条件，而全局 FilterBar 在实时 Tab 不可见，用户切换回主页时容易产生困惑。
- **选项仅合并当前已渲染日志中出现的 dbType** —— 这么做会导致未产生最近请求但已启用的数据源无法被选中；统一使用 `filterStore.providerOptions` 能保持与主页选项一致。

## Consequences

- 换来：实时页筛选与主页数据源语义一致，支持单独收窄特定数据源日志；与全局筛选解耦互不干扰。
- 代价：实时页与主页筛选相互独立，用户在主页切换数据源后进入实时页仍需单独选择（默认显示全部）。
