# DR: TrafficMonitor 与主页面共用 CCS 会话过滤

Status: implemented

## Problem

主页面查询会读取 `ccs_filter_session_enabled` 与
`ccs_filter_session_apps`，排除 CCS 写入的指定终端会话日志。TrafficMonitor
与 macOS 菜单栏复用的今日查询管道此前固定传入 `None`，因此会把这些记录重新计入，
导致常驻展示与主页面的 Token 和请求数不一致。

## Decision

将当前 CCS 会话过滤列表放入 `SharedState`，应用启动时从应用数据库加载，设置页修改时同步更新。
`today_query` 从共享状态读取该配置，并把相同的 `ccs_filter_session_apps` 传给
`compute_precompute`。这样主页面、TrafficMonitor 和菜单栏使用同一过滤口径。

## Alternatives considered

**在每次 TrafficMonitor 请求时重新打开应用数据库读取设置。** 这样可以工作，但会让常驻查询重复创建数据库连接，并把配置读取逻辑分散到后台查询路径；共享状态能在启动与设置变更时一次同步。

**让插件自行过滤 MiniMax 或 CCS 数据。** 插件只能看到汇总接口，无法可靠区分各数据源的会话记录；过滤必须在统一查询管道进入前完成。

## Consequences

- TrafficMonitor 和 macOS 菜单栏会排除与主页面相同的 CCS 会话日志。
- 修改设置后无需重启后台服务，下一次查询即可使用新配置。
- 共享状态新增一个轻量级读写锁，查询时只复制一次过滤列表。
