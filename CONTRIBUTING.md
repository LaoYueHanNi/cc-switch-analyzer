# 贡献指南

感谢你对 CC-Switch Analyzer 的关注！

## 开发环境搭建

### 前置要求

- **Node.js** >= 18
- **pnpm** — 包管理器
- **Rust 工具链** — 通过 [rustup](https://rustup.rs/) 安装

### 克隆与启动

```bash
git clone https://github.com/LaoYueHanNi/cc-switch-analyzer.git
cd cc-switch-analyzer
pnpm install
pnpm dev:tauri
```

首次启动时 Rust 编译约需 1-2 分钟，后续增量编译 5-20 秒。

## 项目结构

```
src/                      # Vue 3 前端
  components/             # UI 组件
  views/                  # 页面视图
  stores/                 # Pinia 状态管理
  composables/            # 组合式函数
  platform/               # 平台适配器（Tauri IPC 封装）
  types/                  # TypeScript 类型
  utils/                  # 工具函数

src-tauri/                # Rust 后端
  src/commands/           # Tauri 命令处理器
  src/services/           # 业务逻辑
  src/models.rs           # 数据模型
  src/utils.rs            # 工具函数

traffic-monitor-plugin/   # TrafficMonitor 插件（C++ 32 位 DLL，仅 Windows）
  CCSwitchAnalyzer.h/cpp  # 插件主类（WinHTTP 请求 + JSON 解析）
  TodayTokenItem.h/cpp    # 显示项（Tokens + Cost）
  PluginInterface.h       # TrafficMonitor 插件接口
  build.bat               # 编译脚本（MSVC x86）
```

## 开发流程

1. Fork 本仓库
2. 创建功能分支：`git checkout -b feature/your-feature`
3. 提交更改：遵循下方的提交规范
4. 推送到你的 Fork：`git push origin feature/your-feature`
5. 创建 Pull Request

## 提交规范

提交信息使用中文，格式：

```
类型: 简短描述

详细说明（可选）
```

类型包括：
- `feat` — 新功能
- `fix` — 修复
- `perf` — 性能优化
- `refactor` — 重构
- `docs` — 文档
- `test` — 测试
- `chore` — 构建/工具

## 代码规范

### 前端（Vue/TypeScript）

- 使用 Composition API + `<script setup>`
- 组件文件名使用 PascalCase
- 遵循现有代码风格，参照同文件中的写法

### 后端（Rust）

- 遵循 Rust 标准命名规范（snake_case）
- 使用 `serde` 进行序列化/反序列化
- 错误处理使用 `Result` 类型

## TrafficMonitor 插件（仅 Windows）

TrafficMonitor 是 Windows 桌面工具，项目包含一个配套的 C++ 插件，用于在任务栏显示今日 Token 用量和费用。

### 前置要求

- **Visual Studio 2022 Build Tools**（MSVC x86 工具链）

### 编译

```bash
cd traffic-monitor-plugin
build.bat
```

产出：`src-tauri/resources/CCSwitchAnalyzer.dll`（通过 `include_bytes!` 内嵌到应用中）

> 修改 `traffic-monitor-plugin/` 下的 C++ 源码后，必须重新执行 `build.bat` 编译 DLL，再重启应用才能生效（DLL 在编译时静态嵌入，不会热加载）。

## 测试

```bash
# 前端测试
pnpm test

# 后端测试
cd src-tauri && cargo test
```

## 铁律

> **禁止修改外部数据源数据库**

CC-Switch、OpenCode 等外部数据库只能进行只读操作（SELECT）。任何涉及修改外部数据库的 PR 将被拒绝。

应用自有数据库（`~/.cc-switch-analyzer/pricing.db`）可正常读写。

## 问题反馈

- 通过 [GitHub Issues](https://github.com/LaoYueHanNi/cc-switch-analyzer/issues) 提交问题
- 请提供复现步骤、预期行为和实际行为
- 如涉及数据库问题，请说明数据来源（CC-Switch / OpenCode）
