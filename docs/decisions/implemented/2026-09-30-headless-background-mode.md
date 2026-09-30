# DR: 后台常驻模式销毁 WebView 窗口只保留 Rust 进程

Status: implemented

## Problem

应用原先的「关闭窗口」行为是 `api.prevent_close()` + `window.hide()`：窗口不可见，但 **WebView 进程完整存活**。

这带来两个问题：

1. **常驻内存不降**。前端是 Vue 3 + Naive UI + ECharts 5.5，WebView 侧渲染引擎（Windows WebView2 / macOS WKWebView）加上页面 JS 堆与 ECharts canvas 是内存大户，而菜单栏两行小字（见 [DR: macOS 菜单栏显示改用原生 NSStatusItem 实现两行小字](../implemented/2026-09-30-native-nsstatusitem-menubar.md)）与 TrafficMonitor HTTP 服务都是**纯后台能力**——用户关掉窗口后并不需要任何前端渲染，却仍在为一个看不见的界面付出内存。
2. **「隐藏」不等于「释放」**。用户对「我已经关掉了」的直觉预期是进程不再做无用功；`hide()` 做不到，还让被隐藏的页面继续跑 `setInterval` 自动刷新与实时轮询（`useAutoRefresh.ts` / `useRealtimePolling.ts`），持续 IPC 拉数、持续 `queryPrecompute`。

一个有利前提是：analyzer 的后台能力**天生与 WebView 解耦**——`services/http_server.rs` 是独立 `std::thread` + 同步 TCP，`services/menubar.rs` 是独立 `menubar-refresh` 线程（30s 轮询），二者都持有独立的 `TodaySourceCache`（见 [DR: macOS 菜单栏显示用原生托盘 title 而非外部宿主插件](../implemented/2026-09-30-macos-menubar-tray-title.md)），不经过前端查询管道。前端侧 `components/layout/AppLayout.vue` 的 `onMounted` 已是完整自恢复入口（`loadCcsSessionFilter()` → `autoLoadDatabase()` → `watch(dbStore.hasDatabase)` 立即触发 `queryPrecompute`），`stores/theme.ts` 与 `stores/updater.ts` 走 `localStorage`。**即前端是无状态可重建的**。

## Decision

新增「后台模式」：进入后台时**销毁** WebviewWindow 而非隐藏，WebView 子进程随之退出，Rust 主进程继续承载托盘/菜单栏与 HTTP 服务；需要界面时按需重建。

### 窗口生命周期

`src-tauri/src/services/headless.rs` 以 `AtomicBool` 记录状态并提供四个入口：

- `enter_headless_mode(app)` — 保存窗口几何 → 切平台外观 → `window.destroy()`
- `exit_headless_mode(app)` — 窗口已存在则 `unminimize/show/set_focus`；否则从 `app.config().app.windows` 找回 `label == "main"` 的配置并 `WebviewWindowBuilder::from_config(app, &cfg)?.build()` 重建，再 `restore_geometry` + 显示
- `show_or_rebuild_window(app)` — 统一唤起入口，内部一律转调 `exit_headless_mode`
- `toggle_headless(app)` — 动作式立即切换

### 五个唤醒路径收敛

托盘左键点击、托盘菜单「显示窗口」、`tauri-plugin-single-instance` 重复启动、macOS `RunEvent::Reopen`（点 Dock）、macOS 菜单栏 `MenuCallbacks.show_window`，全部改调 `show_or_rebuild_window`——原先是五处各写一遍 `get_webview_window("main")?.show()`，后台模式下会静默失败成「点了没反应」。

### 退出流程

`RunEvent::ExitRequested` 中 `code.is_none()`（所有窗口被销毁导致）且处于后台模式时 `api.prevent_exit()` 保持常驻；`code.is_some(_)`（用户主动 `app.exit(0)` / Cmd+Q）置 `should_exit` 标记照常退出。这是 Tauri v2 的必需 workaround——[官方 issue #13511](https://github.com/tauri-apps/tauri/issues/13511) 至今 open，「窗口全销毁后保活进程」无官方一等公民解法。

### 配置与动作分离

刻意拆成两个正交概念，避免「勾了却没反应」：

- **`close_destroys_window`（配置）**：点关闭按钮时销毁还是隐藏。存 `pricing.db::settings`，经 `HeadlessState` 托管
- **后台模式（动作/状态）**：菜单栏「进入后台模式」⇄「退出门面（返回界面）」，标题随状态动态切换，点击立即生效。设置页另有「立即进入后台模式」按钮

设置页的开关只改配置，菜单项给动作——菜单栏就在手边，给配置等于让用户点了没反应。`toggle_headless` 进入后台时会顺带把 `close_destroys_window` 置 true，否则从后台恢复的窗口再点关闭会立刻又被销毁，用户会以为「退不出后台模式」。

### 平台外观

`apply_appearance(app, background)`：macOS 切 `set_dock_visibility` + `ActivationPolicy::Accessory/Regular`；Windows 切 `set_skip_taskbar`。macOS 的 `NSStatusItem` 本身不受影响，故菜单栏两行小字在后台照常刷新。

### 两个关键工程约束

1. **窗口几何自存 `settings` 而非 `tauri-plugin-window-state`**：销毁后 `WebviewWindowBuilder` 动态建窗与启动期建窗路径不同，插件的窗口创建 hook 时序不可靠；改为 destroy 前存 `{w,h,x,y,maximized}`、重建后显式应用。附带 `is_position_visible()` 校验——拔掉外接显示器后旧坐标指向不存在的区域，直接套用会把窗口丢到屏外无法拖回，此时退回 `center()`。
2. **所有 `app.state::<T>()` 一律用 `try_state` 降级**：菜单栏回调跑在主线程，panic 无法 unwind 会直接 `SIGABRT`（开发期实测：`manage` 的是 `HeadlessState`、取的却是 `Mutex<HeadlessState>`，类型不匹配编译期查不出来、运行期整个进程 abort）。这是**编译期不可见、运行期致命**的一类错误，只能靠编码纪律防御。

### 附带处理

- `check-update` 事件：后台时 `emit` 无人接收，改为置 `PENDING_UPDATE` 标志，前端 `AppLayout` 挂载并注册监听后调 `consume_pending_update` 补拉
- `WindowEvent::CloseRequested` 拦截器从 `win.on_window_event`（绑定窗口实例）移到 Builder 级 `on_window_event`——销毁后重建的窗口不受实例级拦截器保护

## Alternatives considered

- **维持现状 `hide()`** —— 零成本、恢复瞬时，但内存一分不省、后台照跑轮询，与要解决的问题正交。
- **保留一个隐藏的最小空窗口做保活锚点**（issue #13511 社区提到的 hack）—— 空 WebView 仍是真实 WebView 进程，内存收益大打折扣，还引入语义混乱的隐藏窗口。
- **等 Tauri 官方支持**（[#13511](https://github.com/tauri-apps/tauri/issues/13511) 截至 2026-09-30 仍 open）—— 无一等公民解法，功能会无限期搁置。
- **拆独立 CLI/headless 二进制** —— 要双份维护数据源/定价/扫描全部逻辑或退化子进程 IPC，且无法复用同一份 `pricing.db`。以「关窗口」为触发点不值这个代价。
- **直接照搬 cc-switch `lightweight.rs`** —— 机制可借鉴，但它依赖 `tauri-plugin-window-state`、且托盘靠 `refresh_tray_menu` 重建菜单刷新勾选态；本项目托盘是 Win/Linux 用 Tauri tray、macOS 用原生 `NSStatusItem` 的双轨结构，须按本项目组织。
- **让后台模式停掉 `http_server` / 菜单栏线程**（认为反正窗口关了）—— 恰恰相反：`http_server` 是 TrafficMonitor 插件的唯一数据来源，菜单栏两行小字更是纯后台能力。销毁 WebView 的前提正是这些服务与 WebView 无关。
- **只给一个勾选式开关管两件事**（首版实现即如此）—— 实测用户勾选后台模式后界面纹丝不动，直觉上「勾了 = 立刻生效」落空。拆分配置与动作后解决。

## Consequences

- 代价：进入后台后重新打开窗口需 0.5–2s 重建，且回到默认路由、筛选条件与已打开的设置面板状态丢失；macOS 上 Dock 图标消失（`Accessory` 策略），需从菜单栏唤起；多出 `close_destroys_window` / `window_geometry` 两个 settings 键。
- 换来：实测销毁后 WebView 相关进程（`com.apple.WebKit.WebContent` + `GPU` + `Networking`）合计 **137.1MB 全部释放**，主进程仅 103.2MB 常驻；菜单栏两行小字与 TrafficMonitor HTTP 服务在后台照常工作。默认仍为「关闭即隐藏」，后台模式是可选项，灰度与回滚零成本。
