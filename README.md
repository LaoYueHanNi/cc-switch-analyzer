# CC-Switch Analyzer

CC-Switch 和 OpenCode 代理使用数据的桌面分析工具。基于 Tauri v2 构建，提供 Token 消耗、模型分布、成本分析等多维度统计。

## 功能特性

### 数据分析

- **按模型统计** - 各 LLM 模型的 Token 消耗、成本分布和对比
- **按供应商统计** - 按供应商维度聚合分析使用情况
- **趋势分析** - 日/小时粒度的 Token 和成本趋势图
- **摘要栏** - 顶部汇总统计（总费用、总 Token、请求数、缓存命中率等）
- **多维筛选** - 按供应商、模型、日期范围快速过滤，支持快捷日期选择

### 会话管理

- **会话分析** - 按项目分组的会话级详细分析，支持密度热力图和模型分解
- **会话标题识别** - 自动从会话记录中提取标题，快速定位目标会话
- **终端启动** - 一键在终端中启动 Claude Code 或 OpenCode，支持新建和恢复会话
- **供应商配置启动** - 右键 Claude 图标可选择 CC-Switch 供应商配置启动终端

### 定价系统

- **自定义定价** - 手动编辑任意模型的输入/输出/缓存读写定价
- **分时定价** - 按时间段设置不同费率，匹配供应商的动态调价
- **上下文阶梯定价** - 按上下文窗口大小设置阶梯费率
- **模型别名** - 为同一模型设置多个别名，适配不同供应商的模型命名
- **模型对比** - 多模型费用对比计算
- **云端定价同步** - 启动时自动拉取云端最新定价数据

### 实时监控

- **实时 Token 监控** - 实时 Token 消耗监控窗口
- **TrafficMonitor 插件** - 在 Windows 任务栏显示今日 Token 消耗和费用（仅 Windows）

### 系统功能

- **多数据源** - 同时支持 CC-Switch 和 OpenCode 数据库
- **深色/亮色模式** - 支持明暗主题切换，配色针对对比度优化
- **系统托盘** - 最小化到系统托盘后台运行
- **自动更新** - 支持应用内检测和安装更新，可配置代理地址

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | Vue 3 + TypeScript 5.5 + Naive UI 2.39 + ECharts 5.5 + Pinia 2 |
| 后端 | Rust 2021 + rusqlite 0.31 + Tauri v2 |
| 插件 | C++17 + WinHTTP（TrafficMonitor 插件） |
| 构建 | pnpm + Vite 5.4 + Cargo |

## 安装

### 从 Releases 下载

前往 [GitHub Releases](https://github.com/LaoYueHanNi/cc-switch-analyzer/releases) 下载对应平台的安装包。

### 从源码构建

**前置要求：**
- Node.js >= 18
- pnpm
- Rust 工具链（rustup）

```bash
# 克隆仓库
git clone https://github.com/LaoYueHanNi/cc-switch-analyzer.git
cd cc-switch-analyzer

# 安装依赖
pnpm install

# 开发模式（热加载）
pnpm dev:tauri

# 生产构建
pnpm build:tauri
```

## 项目结构

```
src/                      # Vue 3 前端
  components/             # UI 组件（charts/, layout/, model/, pricing/, session/）
  views/                  # 页面视图（ByModel, ByProvider, Trend, Session, Realtime, Pricing）
  stores/                 # Pinia 状态管理
  platform/               # 平台适配器抽象层
  composables/            # 组合式函数
  types/                  # TypeScript 类型定义
  utils/                  # 工具函数（format, pricing, color, constants）

src-tauri/                # Rust 后端
  src/commands/           # Tauri 命令处理器（database, pricing, query, session_manager）
  src/services/           # 业务逻辑（pricing_engine, precompute, external_db, app_db, http_server）
  src/models.rs           # 数据模型
  src/utils.rs            # 工具函数

traffic-monitor-plugin/   # TrafficMonitor 插件（仅 Windows）
  CCSwitchAnalyzer.h/cpp  # 插件主类（WinHTTP 请求 + JSON 解析）
  TodayTokenItem.h/cpp    # 显示项（Tokens + Cost）
  build.bat               # MSVC x86 编译脚本
  build64.bat             # MSVC x64 编译脚本
```

## 数据来源

本工具以只读方式访问以下外部 SQLite 数据库：

- **CC-Switch**: `~/.cc-switch/cc-switch.db`（CC-Switch 代理数据库）
- **OpenCode**: 系统数据目录下的 `opencode/opencode.db`

> **重要**: 本工具绝不修改外部数据库，所有访问均为只读（`SQLITE_OPEN_READ_ONLY`）。

## 云端定价数据

应用内置的模型定价数据来自：

- **数据源**: [model-price-table](https://gitee.com/oyw125/model-price-table)（Gitee 公开仓库）
- **更新方式**: 应用启动时自动从云端拉取最新定价

你也可以在应用内手动编辑定价，或使用本地 `model_pricing.json` 文件覆盖。

## 许可证

[MIT License](LICENSE)
