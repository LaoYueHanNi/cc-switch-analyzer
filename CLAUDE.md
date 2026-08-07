# 项目约定

## 项目概述

**CC-Switch Analyzer** — CC-Switch 和 OpenCode 代理使用数据的桌面分析工具。

- **版本**: 0.7.43
- **架构**: Tauri v2（Rust 后端 + Vue 3 前端）
- **仓库**: https://github.com/LaoYueHanNi/cc-switch-analyzer

## 铁律

- **禁止修改外部数据源数据库**：CC-Switch、OpenCode 等外部数据库只做读取操作（SELECT），绝不执行任何 ALTER TABLE、CREATE INDEX、INSERT、UPDATE、DELETE 等修改操作。这是不可违反的约束。

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | Vue 3 + TypeScript 5.5 + Naive UI 2.39 + ECharts 5.5 + Pinia 2 + Vue Router 4 |
| 后端 | Rust 2021 + rusqlite 0.31 + Tauri v2 |
| 插件 | C++17 + WinHTTP（TrafficMonitor 插件，32 位 DLL） |
| 构建 | pnpm + Vite 5.4 + Cargo |

## 目录结构

```
src/                      # Vue 3 前端
  components/             # UI 组件（charts/, layout/, model/, pricing/, provider/, session/）
  views/                  # 页面视图（ByModel, ByProvider, Trend, Session, Realtime, Pricing）
  stores/                 # Pinia stores（database, filter, pricing, query, theme）
  composables/            # 组合式函数
  platform/               # 平台适配器（tauri.ts 封装 Tauri IPC 调用）
  types/                  # TypeScript 类型定义
  utils/                  # 工具函数（format, pricing, color, constants）

src-tauri/                # Rust 后端
  src/commands/           # Tauri 命令处理器（database, pricing, query, session, traffic_monitor）
  src/services/           # 业务逻辑（pricing_engine, precompute, external_db, app_db, http_server）
  src/models.rs           # 数据模型定义
  src/utils.rs            # 工具函数和常量

traffic-monitor-plugin/   # TrafficMonitor 插件（C++ 32 位 DLL）
  CCSwitchAnalyzer.h/cpp  # 插件主类（WinHTTP 请求 + JSON 解析）
  TodayTokenItem.h/cpp    # 显示项（Tokens + Cost）
  PluginInterface.h       # TrafficMonitor 插件接口
  build.bat               # 编译脚本（MSVC x86）
  build64.bat             # 编译脚本（MSVC x64）
```

## 开发命令

```bash
pnpm dev:tauri            # 开发模式（前端 HMR + Rust 增量编译自动重启）
pnpm build:tauri          # 生产构建
pnpm test                 # 前端测试（Vitest）
pnpm test:rust            # 后端测试（cargo test，需在 src-tauri/ 目录）
```

### TrafficMonitor 插件编译（仅 Windows）

> **仅 Windows**：TrafficMonitor 是 Windows 桌面工具，插件 DLL 只在 Windows 下编译和携带，macOS/Linux 不执行此步骤。

**在 Git Bash / Claude Code 环境中**，无法直接 `cmd /c build.bat`，需通过 PowerShell 调用：

```bash
powershell.exe -Command "cd 'D:\Code\oyw\cc-switch-analyzer\traffic-monitor-plugin'; & '.\build.bat'"    # x86
powershell.exe -Command "cd 'D:\Code\oyw\cc-switch-analyzer\traffic-monitor-plugin'; & '.\build64.bat'"   # x64
```

需要 Visual Studio 2022 Build Tools（MSVC）。产出会自动复制到 `src-tauri/resources/` 目录。

> 修改 `traffic-monitor-plugin/` 下的 C++ 源码后，必须重新编译 DLL，再重启应用才能生效（DLL 在编译时通过 `include_bytes!` 静态嵌入）。

## 关键约定

### 平台适配器模式

`src/platform/` 抽象了 Tauri 特定的 IPC 调用。前端通过 `platform/index.ts` 导出的函数与后端通信，不直接调用 Tauri API。

### 版本更新

版本号需同步修改 3 个文件：
1. `package.json` — version 字段
2. `src-tauri/tauri.conf.json` — version 字段
3. `src-tauri/Cargo.toml` — version 字段

使用 `/version` 命令自动处理。

### 数据库访问

- 外部数据库（cc-switch.db、opencode.db）：**只读**，通过 `rusqlite::SQLITE_OPEN_READ_ONLY` 打开
- 应用自有数据库（pricing.db）：可读写，存储用户自定义定价配置

### TrafficMonitor 插件架构

插件通过 HTTP API（`127.0.0.1:19810`）获取今日 token 和费用数据。

- **HTTP 服务**（`http_server.rs`）复用前端查询管道（`compute_precompute`），使用独立的 `DataSource` 实例避免 `rusqlite::Connection` 的 `RefCell` 并发冲突
- **费用由服务端计算**：`totalCost` 返回格式化字符串（如 `"172.57¥"`），插件只做展示不做计算
- **插件 C++ 代码**：通过 `WinHTTP` 请求 `/api/today`，解析 JSON 后直接展示字符串
