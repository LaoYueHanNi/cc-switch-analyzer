# DR: 实时页改为按数据源筛选，表头「延迟」改名为「首字」

Status: implemented

## Problem

实时页工具栏按模型过滤，和主页 FilterBar 的数据源下拉不一致：多源混在一张表里时，用户没法按 CCS / OpenCode / Cursor 等源收窄最近请求。同时表头并排「首字」(TTFT) 和「延迟」(整段 latencyMs)，TTFT 经常空，真正有值、用户要看的是延迟列。

## Decision

实时页工具栏复制主页数据源 CompactSelect（`filterStore.providerOptions`，placeholder「全部」），用独立的 `selectedSource` 按行的 `dbType` 过滤。不绑定 `filterStore.providerId`，避免改实时筛选带动模型/供应商页查询。去掉 TTFT「首字」列，原「延迟」列改名为「首字」，仍展示 `formatLatency(latencyMs)`。

## Alternatives considered

- **绑定 `filterStore.providerId` 与主页共用筛选状态** —— 实时页改源会改掉模型/供应商页的查询条件，FilterBar 在实时 Tab 不可见，用户回来会困惑。
- **选项合并当前日志里出现的 dbType** —— 旧模型下拉这么做是因为模型集合比全局选项更即时；数据源选项已由 `getFilterOptions` 按已加载源给出，再合并会与主页下拉不一致。
- **把「首字」列改成展示 latencyMs、保留「延迟」列** —— 用户明确要求去掉首字、延迟改名，两列并存没有意义。
- **列改名后改用 `formatTtft`** —— 那会把 0ms 显示成 "-"，改变原延迟列的格式化规则；本次只改表头。

## Consequences

- 换来：实时页筛选与主页数据源语义对齐；表头只留一列耗时，标签为「首字」。
- 代价：`timeToFirstToken` 仍从后端返回但实时页不再展示；前端 `formatTtft` 已删，只留 `formatLatency`；「首字」标签对应的是整段 `latencyMs` 而非 TTFT，和字面不完全一致。
