# DR: 筛选选项的填充与日期范围有效性解耦

Status: implemented

## Problem

`getFilterOptions` 一次返回三样东西：数据源列表、模型列表、可查询的日期范围。前端 `useDatabase`
把它们当成一个整体——只有 `dateRange.min > 0` 时才调用 `filterStore.setOptions(...)`，而该调用
同时写入数据源选项、模型选项和日期上下界。

后端 `merge_date_range` 取所有数据源下界的最小值且**不过滤 0**，无记录的数据源返回 `(0, 0)`，
因此**任何一个已启用但当前没有记录的数据源**都会把全局 min 拉成 0。

后果是界面级失效：选项与日期一起被丢弃。顶部统计照常显示「N 个数据源 · M 条记录」，
但「数据源」和「模型」下拉展开只有「无匹配项」，用户看到的是「数据都在、却筛不了」。
触发场景并不罕见：扫描入库型源在记录被清空后尚未重新入库、Cursor 新增账号的 CSV 还没同步、
刚接入的新源首次启动（数据目录存在但还没扫到用量）。

## Decision

把「填充选项」与「日期范围是否有效」拆成两个独立判断：

- 前端 `filterStore` 不再提供一次性写入四者的 `setOptions`，改为 `setSourceOptions(providers, models)`
  与 `setDateRangeBounds(min, max)`；`useDatabase` 的 `updateFilterOptions` 与
  `updateFilterOptionsPreserveDate` 分别按 `providers.length > 0` 和 `dateRange.min > 0` 判断，
  后者保持「不覆盖当前 fromDate/toDate」的原有语义。
- 后端 `merge_date_range` 求下界时忽略 `min <= 0` 的数据源；全部源都无记录时仍返回 `(0, 0)`，
  保留「确实没有可查询范围」的语义。

## Alternatives considered

- **只改后端**：全局下界不再被 0 污染，但前端仍把选项与日期绑在同一个判断里，所有源都暂时无数据时
  选项照样整片丢弃——修不到根因。
- **只改前端**：能挡住这次现象，但 `dateRange.min = 0` 仍会写入 `dateRangeMin/dateRangeMax`，
  FilterBar 的「全部时间」与趋势页会拿到一个无效区间。
- **在 `get_filter_options` 里跳过无记录的数据源**：会掩盖「源已注册但暂无数据」这一事实，
  而用户恰恰需要在下拉里看到该源，才可能去设置页触发扫描。
- **要求每个已启用源都必须有数据**：与扫描入库型源的现实冲突，新接入源的首次启动必然无数据。

## Consequences

- 换来：任一数据源暂时无数据不再影响筛选可用性；「某个源没数据」与「没有筛选选项」两件事解耦。
- 代价：`dateRange` 无效时不再顺带阻止选项填充——这本就不该由它决定；所有源都无记录时，
  下拉会正常列出数据源而查询结果为空，属合理空态。
- 背景：`AppDbService::migrate_v14` 一次性清空 Antigravity 记录曾直接引爆该缺陷（启动后下拉全空），
  但该迁移自身的取舍是另一件事，不在此记录范围。
