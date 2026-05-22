# 项目约定

## 项目概述

**CC-Switch Analyzer** — CC-Switch 和 OpenCode 代理使用数据的桌面分析工具。

- **版本**: 0.6.7
- **架构**: Tauri v2（Rust 后端 + Vue 3 前端）
- **仓库**: https://github.com/LaoYueHanNi/cc-switch-analyzer

## 铁律

- **禁止修改外部数据源数据库**：CC-Switch、OpenCode 等外部数据库只做读取操作（SELECT），绝不执行任何 ALTER TABLE、CREATE INDEX、INSERT、UPDATE、DELETE 等修改操作。这是不可违反的约束。

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | Vue 3 + TypeScript 5.5 + Naive UI 2.39 + ECharts 5.5 + Pinia 2 + Vue Router 4 |
| 后端 | Rust 2021 + rusqlite 0.31 + Tauri v2 |
| 构建 | pnpm + Vite 5.4 + Cargo |
| 测试 | Vitest（前端）+ cargo test（后端） |

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
  src/commands/           # Tauri 命令处理器（database, pricing, query, session）
  src/services/           # 业务逻辑（pricing_engine, precompute, external_db, app_db）
  src/models.rs           # 数据模型定义
  src/utils.rs            # 工具函数和常量
```

## 开发命令

```bash
pnpm dev:tauri            # 开发模式（前端 HMR + Rust 增量编译自动重启）
pnpm build:tauri          # 生产构建
pnpm test                 # 前端测试（Vitest）
pnpm test:rust            # 后端测试（cargo test，需在 src-tauri/ 目录）
```

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
