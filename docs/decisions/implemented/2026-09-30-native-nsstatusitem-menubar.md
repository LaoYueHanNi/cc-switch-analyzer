# DR: macOS 菜单栏显示改用原生 NSStatusItem 实现两行小字

Status: implemented

## Problem

[前一决策](2026-09-30-macos-menubar-tray-title.md)用 `TrayIcon::set_title` 在菜单栏显示今日数据，但该 API 只支持纯文本单行系统字体（tray-icon crate 无字体/字号/换行定制能力）。实际体验后需要 iStat Menus 风格的显示：字号更小、Token 与费用上下两行，set_title 无法满足。

## Decision

macOS 端不再创建 Tauri 托盘，改用 `objc2`/`objc2-app-kit` 直接构建原生 `NSStatusItem`（`services/menubar_macos.rs`）：

- 按钮内容 = 应用图标（模板模式，深浅色自适应）+ 两行 9pt 等宽数字字体（`monospacedDigitSystemFont`，上行 Token k/M 缩写（`88.86M`）、下行费用（`33.98¥`，符号后置以与 Token 的「数值+单位」排版一致），两者统一固定两位小数，并按数值部分右对齐、单位同列排版（整数位数不同时以空格补齐，配合段落左对齐使前导空格生效））
- **内容整体位图化**：图标与两行文字先在离屏位图上合成一张模板图，再作为 `NSStatusItem` 的唯一 `image`，由 AppKit 居中（实测 `imageRect` 上下留白严格对称）。原因是 `NSStatusBarButtonCell` 对多行 `title` 是**顶对齐而非居中**（macOS 27 实测：按钮 24pt 高、文字块中心被排到 30pt，整块偏上 18pt），且缩小字号只会让底部空得更多、顶边纹丝不动；`attributedTitle` 的 `baselineOffset` 同样被 cell 忽略（实测文字纹丝不动）。位图化后排版完全自控，深浅色由模板图自动适配
- **文本绘制走 CoreText 而非 AppKit 分类**：objc2-app-kit 0.3.2 未为 `NSAttributedString` 生成 AppKit 分类方法（`size`/`drawWithRect:`），经 `msg_send!` 调用在本机 arm64 上结构体传参与返回值均不可靠（实测 `size` 返回 8.03×12 垃圾值、`drawWithRect:` 直接抛 ObjC 异常导致 abort）。改用 `CTFramesetter`（`suggest_frame_size_with_constraints` 测量 + `CTFrame::draw` 绘制）这类纯 C 函数，ABI 明确可靠。注意 `CFRange.length` 是 UTF-16 码元数，须用 `NSAttributedString.length()` 而非 UTF-8 字节数，否则范围越界使 `CTFramesetterCreateFrame` 返回 NULL
- **位图上下文不做 CTM 翻转**：`CTFrameDraw` 会自行处理上下文方向，额外翻转会把文字上下颠倒
- **段落取左对齐而非居中**：两行数字位数可能不同（`345.60k` vs `33.98¥`），靠给短行数值前补空格实现数字右对齐与单位同列；居中会吞掉行首空格导致失效
- 交互：左键唤起主窗口，右键弹出原生 `NSMenu`（显示窗口 / 启用菜单栏显示（勾选态）/ 检查更新 / 退出）；`sendActionOn(LeftMouseUp|RightMouseUp)` 把两种按键都引入统一 action，按 `currentEvent.type` 分流
- 自定义 ObjC 类 `MenuTarget`（`define_class!`）作为 action target，经静态 `MenuCallbacks` 闭包回调 Rust 逻辑
- `menubar.rs` 刷新线程不变（30s 轮询共享查询管道），更新文字经 `AppHandle::run_on_main_thread` 进入主线程（AppKit 状态项要求），公开函数以 `MainThreadMarker` 参数为证
- Tauri 托盘保留给 Windows/Linux（`#[cfg(not(target_os = "macos"))]`）；`capabilities` 托盘权限与前端适配层不变

objc2 系依赖（objc2 0.6 / objc2-app-kit 0.3.2 / objc2-foundation 0.3 / objc2-core-text 0.3 / objc2-core-graphics 0.3 / objc2-core-foundation 0.3）为 macOS target 专属直接依赖，版本锁定与 Tauri 依赖树内一致，不引入重复编译。

## Alternatives considered

- **维持 `set_title` 单行** —— 字体、字号、换行均不可控，明确不满足需求。
- **并列第二个纯文字 NSStatusItem** —— 保留 Tauri 托盘可少一半 objc 代码，但菜单栏出现两个相邻图标、视觉割裂。
- **SwiftBar 插件** —— 两行小字样式可完全自定义，但要求用户额外安装 SwiftBar 且需放开 HTTP 服务常驻，作为日常默认形态门槛过高。
- **fork tray-icon 增加 attributed title 支持** —— 为一个展示需求背上上游维护负担，且 tauri 对 tray-icon 的 minor 版本联动会放大耦合。
- **用 `attributedTitle` + `baselineOffset` 补偿垂直位置** —— 改动最小，理论可行（运行时读 `titleRect` 算中心差再平移），但实测 `NSStatusBarButtonCell` 绘制时忽略该属性，文字纹丝不动，已放弃
- **自定义 `NSView` 自绘** —— 同样能绕开 cell 排版，但要重写 `mouseDown`/`rightMouseDown` 接管现有左键/右键交互，改动面比位图化大；当前位图化已满足排版需求，暂不引入
- **文本测量/绘制走 AppKit 分类（`-[NSAttributedString size]` / `drawWithRect:`）** —— 写法最直白，但 objc2-app-kit 0.3.2 未生成这些方法，`msg_send!` 在本机 arm64 上返回垃圾结构体 / 直接抛异常 abort（用 Swift 写同样逻辑完全正常，可确认是 objc2 侧问题），故改用 CoreText 的 C 函数

## Consequences

- 代价：引入 ~300 行 unsafe ObjC 互操作代码，与 objc2 0.6 的宏/API 形态耦合（升级 objc2 需回归菜单栏）；菜单事件从 muda 统一事件改为自定义 ObjC target 桥接；文字成为位图（菜单栏小字号 @2x 下清晰，但不再随系统字号辅助功能设置缩放）；release 编译时间略增。
- 换来：iStat 风格两行小字（9pt 等宽数字，宽度可控不挤压菜单栏）且**垂直居中可精确控制**（不再受 cell 排版缺陷影响）；交互完全原生（左键/右键分流、勾选态菜单项）；深浅色全自适应。
