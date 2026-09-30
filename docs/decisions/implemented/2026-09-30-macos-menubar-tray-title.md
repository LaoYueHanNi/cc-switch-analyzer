# DR: macOS 菜单栏显示用原生托盘 title 而非外部宿主插件

Status: implemented

## Problem

Windows 端已有 TrafficMonitor 插件（本地 HTTP API + C++ DLL）在任务栏展示今日 Token 与费用，macOS 无对应能力。前期调研确认 macOS 不存在"通用监控宿主 + 插件 SDK"生态：TrafficMonitor 无 mac 版，Stats / iStat Menus 无第三方插件 API，唯一可复用的脚本宿主是 SwiftBar/xbar（要求用户额外安装）。用户需求是"在状态栏显示总 token 和费用即可"。

## Decision

> [!NOTE]
> 本决策中的「菜单栏 title 文本显示实现」已于 2026-09-30 由 [DR: macOS 菜单栏显示改用原生 NSStatusItem 实现两行小字](2026-09-30-native-nsstatusitem-menubar.md) 部分取代：`TrayIcon::set_title` 纯文本方案升级为原生 `NSStatusItem` 两行小字；本决策关于「原生托盘而非外部宿主插件」「共享查询管道抽取」「按平台各走最优路径」的核心取舍继续有效。

macOS 用应用自身托盘的 `TrayIcon::set_title`（NSStatusItem 原生能力）常驻显示今日数据：`services/menubar.rs` 起一个 `menubar-refresh` 线程，每 30s 调共享查询管道取今日 Token 总量与费用，格式化为紧凑标题（如 `345.60k 12.34¥`，Token 用 k/M 缩写控制在 ~12 字符防菜单栏截断）写入托盘 title；查询失败或无数据源时显示 `—`。设置键 `menubar_display_enabled` 持久化开关（`pricing.db::settings`，同 `tm_service_enabled` 模式），应用启动时恢复；开关命令 `get_menubar_status` / `toggle_menubar_display` 仅在 macOS 生效（非 macOS 返回 `supported: false` 或报错，前端据此隐藏区块）。与"关闭窗口隐藏到托盘"的既有常驻模式配合，托盘图标 + 菜单保持不变，仅追加 title 文本（模板图标 + 系统渲染文本，深浅色自动适配）。

支撑重构：原 Windows-only `http_server.rs` 内嵌的今日查询管道（`query_today_data`、`TmSourceCache`、`TmTodayData`）抽为全平台共享模块 `services/today_query.rs`，HTTP 服务与菜单栏各自持有独立的 `TodaySourceCache`（长驻 DataSource 缓存，锁不共享、互不阻塞），`TodayData` 增加 `#[serde(skip)] total_cost_value: f64` 供菜单栏自行格式化，HTTP JSON 字段与格式完全不变。

## Alternatives considered

- **SwiftBar 插件脚本**（macOS 最接近 TrafficMonitor 的宿主生态）—— 要求用户 `brew install swiftbar`，且需放开 HTTP 服务到 macOS 常驻；作为主方案门槛过高，留作未来可选补充。
- **独立 Swift 菜单栏小程序**（sidecar 或独立 app）—— 引入 swiftc 构建链、签名公证与独立进程维护，为展示两个数字不值得。
- **继续轮询 HTTP API 而非进程内直调** —— 菜单栏与主进程同进程，绕道 localhost HTTP 毫无收益，还引入端口占用与 19810-19820 漂移问题。
- **Windows 也用托盘 title 统一两端** —— `tray-icon` 的 `set_title` 在 Windows 明确不支持（Unsupported），Windows 继续由 TrafficMonitor 插件覆盖。

## Consequences

- 代价：`http_server.rs` 的查询逻辑搬家（行为不变、纯抽取）；菜单栏数据刷新要求主进程存活（Cmd+Q 后停止，与 HTTP 服务同一约束）；30s 刷新粒度不做推送。
- 换来：macOS 零依赖、零安装门槛的状态栏显示；今日查询管道全平台共享，未来 Linux 等平台可直接复用；设置页两个插件区块按平台（`supported` 字段）各显各的，macOS 不再显示无效的 Windows 下载按钮。
