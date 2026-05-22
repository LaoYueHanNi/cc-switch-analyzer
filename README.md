# CC-Switch Analyzer

CC-Switch 和 OpenCode 代理使用数据的桌面分析工具。基于 Tauri v2 构建，提供 Token 消耗、模型分布、成本分析等多维度统计。

## 功能特性

- **按模型统计** - 各 LLM 模型的 Token 消耗和成本分布
- **按供应商统计** - 按供应商维度聚合分析
- **趋势分析** - 日/小时粒度的 Token 成本趋势图
- **会话分析** - 按项目分组的会话级详细分析
- **实时监控** - 实时 Token 消耗监控窗口
- **定价计算器** - 支持自定义定价、分时定价、上下文阶梯定价
- **多数据源** - 同时支持 CC-Switch 和 OpenCode 数据库
- **深色模式** - 支持明暗主题切换
- **系统托盘** - 最小化到系统托盘

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | Vue 3 + TypeScript + Naive UI + ECharts + Pinia |
| 后端 | Rust + rusqlite + Tauri v2 |
| 构建 | pnpm + Vite + Cargo |

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
  components/             # UI 组件（布局、图表、模型、定价等）
  views/                  # 页面视图
  stores/                 # Pinia 状态管理
  platform/               # 平台适配器抽象层
  router/                 # Vue Router 配置
  types/                  # TypeScript 类型定义
  utils/                  # 工具函数

src-tauri/                # Rust 后端
  src/commands/           # Tauri 命令处理器
  src/services/           # 业务逻辑（定价引擎、数据库服务等）
  src/models.rs           # 数据模型
  src/utils.rs            # 工具函数

docs/                     # 文档（PRD、开发计划）
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
