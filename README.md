# CC-Switch Analyzer

多数据源 AI 用量桌面分析工具，基于 Tauri v2、Rust 和 Vue 3 构建。支持 CC-Switch、OpenCode、AI-Proxy、Cursor、ZCode、Proma、DSH、MiniMax Code、Antigravity、Kimi 与 PI（pi / oh-my-pi），提供 Token、费用、模型分布、会话与实时请求分析。

**当前代码版本：0.8.8**。多数据源可同时接入，合并查询并按请求去重。

## 功能特性

### 数据分析

- **按模型 / 供应商统计**：查看 Token 消耗、费用和缓存命中率，支持多模型对比。
- **趋势分析**：按日、小时、星期查看用量与费用趋势，支持按模型对比和 Top N 提示。
- **摘要与筛选**：顶部汇总费用、Token、请求数等指标；按数据源、供应商、模型和日期筛选。
- **多源合并**：各数据源可独立启用 / 禁用，合并时去重，并利用增量扫描与缓存减少重复读取。
- **CCS 会话同步过滤**：可排除指定终端的会话同步记录，保留代理转发记录，减少与本地数据源的重复统计；主页面与常驻显示共用过滤配置。
- **本地源历史保留**：扫描结果存入应用自身数据库；ZCode 已导入用量不随源库的 30 天裁剪消失。

### Cursor 用量

- **CSV 同步**：使用 Session Token 登录并从 Cursor API 同步用量，支持北京时间窗口（1 天 / 7 天 / 30 天 / 全部）。
- **后台缓存展示**：轻量模式只展示已下载用量，允许滞后；需要最新数据时手动同步。前台自动同步保留 24 小时有效期，后台不额外请求 Cursor API。
- **多账号隔离**：按账号缓存，多账号可同时查询，仅当前绑定账号可同步；退出登录默认保留缓存。
- **本机精准归因**：每个账号可独立开关；按本机 Hook 日志的分钟窗（±5）与模型家族匹配，可设置归因起始时间并行级改判。
- **CSV 预览与 Hook 运维**：分页预览、按模型筛选、查看过滤原因；支持 Hook 日志备份、归整压缩和异常预警。
- **聚合用量计价**：Cursor 聚合记录使用基础定价与分时规则，避免把聚合 Token 误判为单次请求的高上下文档位。

### 会话与任务

- **会话分析**：按项目查看会话统计、热力图和模型分解；支持会话管理的数据源会补充标题与项目路径。
- **Codex / Grok 元数据**：辅助识别真实会话、标题和工作目录；Grok 本地文件用于元数据补充，用量来自已接入的用量源。
- **终端启动与恢复**：Windows / macOS 支持 Claude Code、OpenCode、Codex、Grok Build；Claude 可选择 CC-Switch 供应商配置启动。
- **任务 Tab**：跨目录、跨 agent 聚合会话，并按分屏规则打开支持恢复的会话。
- **Kimi / PI 会话**：导入 Kimi CLI / Code / Work、pi / oh-my-pi 的会话信息，包含 OMP 桌面版 profiles。

各数据源的会话、项目归属能力不同；能统计用量不代表支持一键恢复该终端。

### 定价系统

- **自定义定价与别名**：编辑输入、输出、缓存读写价格，配置模型别名；缺定价模型可从模型卡片原地设置。
- **分时与上下文阶梯**：支持时间段、星期 / 工作日规则与上下文窗口阶梯费率。
- **带入真实用量**：hover 模型卡片时显示“带入定价”，一键把四项 Token 填入定价 Tab。
- **三种展示模式**：家族分组、全部模型卡片和紧凑排行榜；按模拟费用或模型名排序，用箭头切换升降序。
- **Token 单位自适应**：四项各自按量显示 K / M / B。手动输入结束后（失焦或 Enter）自动按 `1000 K = 1 M`、`1000 M = 1 B` 进位；输入期间保留原文，清空立即回到 K。
- **模型费用比较**：用同一组实际 Token 模拟不同模型费用，显示单位换算不改变计算数量。
- **云端同步**：启动时获取云端模型定价，也可使用本地覆盖。

### 实时监控与后台常驻

- **实时请求页**：按数据源展示请求，支持源筛选、会话分组和增量刷新；分别查看首字时间、请求耗时、输出 / 总速度，速度两部分可分别隐藏。指标以源数据为准，缺失时显示占位。
- **Windows TrafficMonitor**：从设置下载 x86 / x64 插件，并启用本地 HTTP 服务，在任务栏展示今日 Token 与费用；每次请求读取当前统计，服务换端口后插件自动重试发现。
- **macOS 菜单栏**：原生两行小字显示今日 Token 与费用，每轮查询后等待 5 秒刷新，适配深色 / 亮色外观。
- **后台模式**：可立即销毁界面，仅保留 Rust 进程与常驻服务；再次打开时重建窗口。也可配置关闭窗口时自动进入后台模式。
- **系统托盘与更新**：默认关闭窗口隐藏到托盘，支持单实例唤起、明暗主题和应用内自动更新，可配置 GitHub 更新代理。

后台模式重建界面时恢复窗口位置与大小，页面与筛选回到默认状态。退出应用会停止常驻服务。

本地用量采集由 Rust 持续运行：CCS、OpenCode、AI-Proxy 直接只读查询；ZCode、Proma、DSH、MiniMax、Antigravity、Kimi、PI 每轮扫描后等待 5 秒，按已启用的来源导入应用库，销毁界面后继续采集。通常目标为数据可读后约 5–10 秒加扫描 / 查询耗时更新到状态栏，Windows 另受 TrafficMonitor 的轮询周期影响；Antigravity 需要 IDE 的 language server 可用。Cursor 保持已有缓存展示，需要最新用量时手动同步。

单个数据源或采集失败（如 Antigravity 未打开）时，Windows 插件仍展示其余来源的今日数字；全部来源失败才显示 `--`。macOS 全部失败显示 `—`，部分数据加 `*` 提示不完整。时区使用本机自然日，兼容半小时偏移与夏令时。修复与已验证范围见 [轻量模式审计](docs/audits/2026-10-10-lightweight-mode.md)。

## 数据来源

外部数据库只读打开；本地文件 / RPC 用量只写入分析器自己的缓存或数据库。支持的数据源如下，`~` 表示当前用户主目录：

| 数据源 | 默认路径 / 采集方式 |
|--------|--------------------|
| **CC-Switch（CCS）** | `~/.cc-switch/cc-switch.db`，读取代理与会话同步用量 |
| **OpenCode** | 默认 `~/.local/share/opencode/opencode.db`，也可在设置中指定实际数据库 |
| **AI-Proxy** | 默认 `~/.ai-agent-tools/data/access_log.db`，可选择其他本地数据库 |
| **Cursor** | API 同步 CSV 到 `~/.cc-switch-analyzer/cursor-cache/<userId>/`，本机 Hook 用于归因 |
| **ZCode** | 只读 `~/.zcode/cli/db/db.sqlite`，增量保存精简用量到应用库 |
| **Proma** | `~/.proma/agent-sessions/*.jsonl`，结合会话 / 工作区索引导入 |
| **DSH** | 默认扫描 `~/.dsh/sessions/` 的 JSONL / zstd；可切换到 dsh-token-usage 插件的按天 JSONL |
| **MiniMax Code（Mavis）** | 优先只读 `~/.minimax/v2/sqlite/runtime-state.sqlite`；缺少 SQLite 时回退 `~/.minimax/v2/sessions/` 的旧版 JSONL |
| **Antigravity** | 从 `~/.gemini/antigravity*` 发现会话，向运行中的 language server 读取用量并缓存 |
| **Kimi** | Kimi CLI 的 `~/.kimi/sessions/`、Kimi Code 的 `~/.kimi-code/sessions/`，以及 Kimi Work 桌面缓存目录 |
| **PI（pi / oh-my-pi）** | `~/.pi/agent/sessions/`、`~/.omp/agent/sessions/`、`~/.omp/profiles/<profile>/agent/sessions/` |

DSH 插件目录可在设置中选择，默认按 `$DSH_HOME/token-usage` 或 `~/.dsh/token-usage` 解析。PI 支持 `PI_CODING_AGENT_DIR` 与 `PI_CONFIG_DIR`，Kimi Code 支持 `KIMI_CODE_HOME` 覆盖默认目录；设置页列出实际发现的目录。

Kimi Work 默认会话位于 macOS 的 `~/Library/Application Support/kimi-desktop/daimon-share/daimon/runtime/kimi-code/home/sessions/` 或 Windows 的 `%APPDATA%/kimi-desktop/daimon-share/daimon/runtime/kimi-code/home/sessions/`，并支持桌面配置中的 `shareDir`。

Grok 会话标题和项目路径从 `$GROK_HOME` 或 `~/.grok/{sessions,archived_sessions}/` 的元数据补充。Antigravity 采集新用量时需要其 IDE / language server 正在运行，已导入历史在关闭 IDE 后仍可查询。

**数据保护**：绝不修改 CC-Switch、OpenCode、AI-Proxy、ZCode、MiniMax 等外部数据库。应用自有库 `~/.cc-switch-analyzer/pricing.db` 保存定价、设置、任务与扫描型源的派生数据；Cursor 缓存按账号隔离。

## 安装与构建

### 从 Releases 下载

前往 [GitHub Releases](https://github.com/LaoYueHanNi/cc-switch-analyzer/releases) 下载 Windows 安装包或 macOS 安装包。当前 CI 构建 Windows MSI 和 macOS DMG，并生成应用内更新清单。

### 从源码构建

需要 Node.js、pnpm 和 Rust stable 工具链；CI 使用 Node.js 20 与 pnpm 10.26。Windows 需要 MSVC 构建工具及 WebView2 Runtime，macOS 需要 Xcode Command Line Tools。平台依赖见 [Tauri 开发环境说明](https://v2.tauri.app/start/prerequisites/)。

```bash
git clone https://github.com/LaoYueHanNi/cc-switch-analyzer.git
cd cc-switch-analyzer
pnpm install

# Tauri 开发模式（前端热加载 + Rust 增量编译）
pnpm dev:tauri

# 生产构建与安装包
pnpm build:tauri

# Windows 免安装 EXE（跳过安装包，仍包含生产前端）
pnpm tauri build --no-bundle
```

Windows 免安装产物为 `src-tauri/target/release/cc-switch-analyzer.exe`，运行时需要系统已有 WebView2 Runtime。macOS 安装包位于 `src-tauri/target/release/bundle/dmg/`。

### 验证与前端开发

以下命令从仓库根目录执行：

```bash
pnpm dev:tauri-frontend    # 仅 Vite 前端
pnpm build:tauri-frontend  # TypeScript 检查 + 前端构建
pnpm test                  # Vitest
pnpm test:rust             # 自动进入 src-tauri 运行 cargo test
node docs/decisions/check.mjs # 决策记录校验
```

### TrafficMonitor 插件编译（仅 Windows）

插件已有 x86 / x64 DLL，可从应用设置页下载。修改插件 C++ 源码时，使用 Visual Studio 2022 Build Tools，在仓库根目录的 PowerShell 中执行：

```powershell
Push-Location traffic-monitor-plugin
.\build.bat
.\build64.bat
Pop-Location
```

脚本将产出复制到 `src-tauri/resources/`，随后重新构建 Tauri 应用以嵌入新 DLL。macOS / Linux 不执行插件编译。

## 技术栈与项目结构

| 层级 | 技术 |
|------|------|
| 前端 | Vue 3.5 + TypeScript 5.9 + Naive UI 2.44 + ECharts 5.6 + Pinia 2.3 + Vue Router 4.6 |
| 后端 | Rust 2021 + rusqlite 0.31 + ureq 3 + zstd + Tauri v2 |
| macOS 菜单栏 | objc2 + AppKit / Core Text |
| Windows 插件 | C++17 + WinHTTP，x86 / x64 DLL |
| 构建与测试 | pnpm + Vite 5.4 + Cargo + Vitest |

依赖的精确版本见 `pnpm-lock.yaml` 与 `src-tauri/Cargo.lock`。

```text
src/
  components/             # common、charts、layout/settings、model、pricing 等
  views/                  # 模型、供应商、趋势、会话、实时、定价、任务
  stores/                 # Pinia 状态管理
  platform/               # Tauri IPC 适配器与契约类型
  composables/            # 加载、定价、会话、轮询与 Token 输入逻辑
  types/ / utils/         # 数据类型与工具

src-tauri/
  src/commands/           # IPC 命令
  src/services/           # 数据源、扫描器、查询管道、会话与常驻服务
  src/lib.rs              # 应用入口与共享状态
  resources/              # 静态嵌入的 Windows 插件 DLL

traffic-monitor-plugin/   # TrafficMonitor 插件源代码与编译脚本
tests/frontend/           # 前端测试
docs/decisions/            # 决策记录
changelog-site/            # 变更日志生成与展示
.github/workflows/        # Windows / macOS 发布构建
```

开发约定见 [AGENTS.md](AGENTS.md) 与 [CLAUDE.md](CLAUDE.md)。非平凡变更需携带 [决策记录](docs/decisions/README.md)；项目约定、版本说明与用户功能文档随代码同步维护。

## 云端定价数据

模型定价来自 [model-price-table](https://gitee.com/oyw125/model-price-table)（Gitee 公开仓库），应用启动时拉取最新数据。可在应用内编辑模型定价并保存本地覆盖。

模型识别依次匹配模型 ID、历史／本地精确别名、云端正则别名，均忽略 ASCII 字母大小写。正则要求完整名称匹配，保留 `fast`、`mini` 等模型身份；未知或存在歧义的名称不会推测价格。别名管理可查看云端正则规则，并添加本地精确别名；下载失败时继续使用已缓存的定价。

## 许可证

[MIT License](LICENSE)
