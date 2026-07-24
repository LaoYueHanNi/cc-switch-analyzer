# CC-Switch Analyzer

CC-Switch、OpenCode、AI-Proxy 与 Cursor 用量数据的桌面分析工具。基于 Tauri v2 构建，提供 Token 消耗、模型分布、成本分析等多维度统计；多数据源可同时接入，透明合并查询并做请求级去重。

## 功能特性

### 数据分析

- **按模型统计** - 各 LLM 模型的 Token 消耗、成本分布和对比
- **按供应商统计** - 按供应商维度聚合分析使用情况
- **趋势分析** - 按日 / 小时 / 星期查看 Token 与费用趋势；支持按模型对比模式（汇总 tip 可展开 Top N）
- **摘要栏** - 顶部汇总统计（总费用、总 Token、请求数、缓存命中率等）
- **多维筛选** - 按供应商、模型、日期范围快速过滤，支持快捷日期选择
- **多源合并** - 并行查询各数据源，请求级去重与增量缓存，避免跨代理重复计数

### Cursor 用量

- **CSV 同步** - 使用 Session Token 登录后从 Cursor API 同步用量 CSV；可配置北京时间同步窗口（1 天 / 7 天 / 30 天 / 全部）
- **多账号隔离** - 按账号本地分目录缓存；多账号可同时查询，仅当前绑定账号可同步；退出登录默认保留已下载 CSV
- **本机精准归因** - 用本机 Hook 日志按分钟窗（±5）+ 模型家族过滤 CSV；归因起始时间可配；支持行级申诉改判
- **CSV 预览** - 分页预览用量明细，支持按模型筛选、查看过滤原因与改判
- **Hook 运维** - 同步后可备份 `requests.jsonl`；支持归整压缩；Hook 写入异常时工具栏红灯预警

### 会话与任务

- **会话分析** - 按项目分组的会话级详细分析，支持密度热力图和模型分解
- **会话标题识别** - 多来源解析会话标题，快速定位目标会话
- **Codex 会话** - 识别 Codex 请求并按真实 session 聚合；OpenAI API token 归一化
- **终端启动** - 一键启动 / 恢复 Claude Code、OpenCode、Codex（Windows / macOS）
- **供应商配置启动** - 右键 Claude 图标可选择 CC-Switch 供应商配置启动终端
- **任务 Tab** - 跨目录、跨 agent 聚合多个会话；一键按分屏规则打开全部会话

### 定价系统

- **自定义定价** - 手动编辑任意模型的输入/输出/缓存读写定价
- **分时定价** - 按时间段设置不同费率，匹配供应商的动态调价
- **上下文阶梯定价** - 按上下文窗口大小设置阶梯费率
- **模型别名** - 为同一模型设置多个别名，适配不同供应商的模型命名
- **模型家族分组** - 定价 Tab 按云端模型家族分组展示
- **模型对比** - 多模型费用对比计算
- **云端定价同步** - 启动时自动拉取云端最新定价数据

### 实时监控

- **实时 Token 监控** - 实时 Token 消耗监控窗口；Codex 请求按真实 session 聚合
- **TrafficMonitor 插件** - 在 Windows 任务栏显示今日 Token 消耗和费用；支持插件下载、本地 HTTP 服务与端口自动发现（仅 Windows）

### 系统功能

- **多数据源** - 同时支持 CC-Switch、OpenCode、AI-Proxy、Cursor；各源可独立启用 / 禁用并持久化
- **深色/亮色模式** - 支持明暗主题切换，配色针对对比度优化
- **系统托盘** - 最小化到系统托盘后台运行；单实例，重复启动激活已有窗口
- **自动更新** - 应用内检测和安装更新，可配置代理地址访问 GitHub

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
  components/             # UI 组件（charts/, layout/, model/, pricing/, session/, task/）
  views/                  # 页面（ByModel, ByProvider, Trend, Session, Realtime, Pricing, Task）
  stores/                 # Pinia 状态管理
  platform/               # 平台适配器抽象层
  composables/            # 组合式函数
  types/                  # TypeScript 类型定义
  utils/                  # 工具函数

src-tauri/                # Rust 后端
  src/commands/           # Tauri 命令（database, pricing, query, session, cursor, …）
  src/services/           # 业务逻辑（pipeline, pricing_engine, cursor_*, ai_proxy_db,
                          #           codex_sessions, external_db, app_db, http_server, …）
  src/models.rs           # 数据模型
  src/utils.rs            # 路径与缓存工具

traffic-monitor-plugin/   # TrafficMonitor 插件（仅 Windows）
  CCSwitchAnalyzer.h/cpp  # 插件主类（WinHTTP 请求 + JSON 解析）
  TodayTokenItem.h/cpp    # 显示项（Tokens + Cost）
  build.bat / build64.bat # MSVC x86 / x64 编译脚本
```

## 数据来源

本工具访问以下数据源（外部 SQLite **只读**；Cursor 为本机缓存）：

| 数据源 | 典型路径 / 说明 |
|--------|----------------|
| **CC-Switch** | `~/.cc-switch/cc-switch.db` |
| **OpenCode** | 系统数据目录下的 `opencode/opencode.db` |
| **AI-Proxy** | 本地 AI-Proxy SQLite（设置中可选） |
| **Cursor** | `~/.cc-switch-analyzer/cursor-cache/<userId>/`（API 同步的 `usage.csv`）；本机 Hook 日志用于精准归因 |

> **重要**: 绝不修改 CC-Switch / OpenCode / AI-Proxy 等外部数据库（`SQLITE_OPEN_READ_ONLY`）。Cursor 仅写入应用自身缓存与凭证目录，不改 Cursor 源文件。

## 云端定价数据

应用内置的模型定价数据来自：

- **数据源**: [model-price-table](https://gitee.com/oyw125/model-price-table)（Gitee 公开仓库）
- **更新方式**: 应用启动时自动从云端拉取最新定价

你也可以在应用内手动编辑定价，或使用本地 `model_pricing.json` 文件覆盖。

## 许可证

[MIT License](LICENSE)
