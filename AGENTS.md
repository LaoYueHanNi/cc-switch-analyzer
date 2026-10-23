# 项目约定

## 项目概述

**CC-Switch Analyzer** — 多数据源 AI 用量桌面分析工具，支持 CC-Switch、OpenCode、AI-Proxy、Cursor、ZCode、Proma、DSH、MiniMax Code、Antigravity、Kimi、PI（pi / oh-my-pi）与 Magpie。

- **当前代码版本**: 0.8.12（以 `package.json`、`src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml` 为准）
- **架构**: Tauri v2（Rust 后端 + Vue 3 前端）
- **仓库**: https://github.com/LaoYueHanNi/cc-switch-analyzer
- **用户功能与数据路径**: [README.md](README.md)

## 铁律

- **禁止修改外部数据源数据库**：CC-Switch、OpenCode、AI-Proxy、ZCode、MiniMax 等外部数据库只做读取操作（SELECT），绝不执行任何 ALTER TABLE、CREATE INDEX、INSERT、UPDATE、DELETE 等修改操作。这是不可违反的约束。扫描结果只写入应用自有的 `pricing.db`。
- **非平凡变更须在同一个提交里新增或更新 docs/decisions/ 下的决策记录，规范见 [docs/decisions/README.md](docs/decisions/README.md)**

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | Vue 3.5 + TypeScript 5.9 + Naive UI 2.44 + ECharts 5.6 + Pinia 2.3 + Vue Router 4.6 |
| 后端 | Rust 2021 + rusqlite 0.31 + ureq 3 + zstd 0.13 + Tauri v2 |
| macOS 菜单栏 | objc2 + AppKit / Core Text（原生 NSStatusItem） |
| Windows 插件 | C++17 + WinHTTP（TrafficMonitor，32/64 位 DLL） |
| 构建与测试 | pnpm + Vite 5.4 + Cargo + Vitest |

前端依赖的精确版本以 `pnpm-lock.yaml` 为准，Rust 依赖以 `src-tauri/Cargo.lock` 为准。

## 目录结构

```text
src/                      # Vue 3 前端
  components/             # common/, charts/, layout/settings/, model/, pricing/,
                          # provider/, session/, task/ 等 UI 组件
  views/                  # ByModel, ByProvider, TrendAnalysis, SessionAnalysis,
                          # RealtimeToken, PricingCalculator, Task, TaskDetail
  stores/                 # database, filter, pricing, query, task, theme, updater
  composables/            # 数据加载、定价覆盖、会话恢复、实时轮询、Token 输入等
  platform/               # index.ts 导出适配器；tauri.ts 封装 IPC；types.ts 定义契约
  types/                  # 前端数据类型
  utils/                  # 格式化、定价、模型家族、颜色等工具

src-tauri/
  src/commands/           # database, cursor, pricing, query, session_manager,
                          # session_title, task, traffic_monitor, menubar, headless
  src/services/           # DataSource 与查询管道、各源 *_db / *_scanner、
                          # 会话元数据、today_query、http_server、menubar、headless
  src/lib.rs              # 应用入口、共享状态、托盘与 IPC 注册
  src/models.rs           # Rust 数据模型
  src/utils.rs            # 默认路径、时间与缓存工具
  resources/              # Windows 插件 DLL（构建时嵌入）

traffic-monitor-plugin/   # TrafficMonitor C++ DLL（仅 Windows）
  CCSwitchAnalyzer.h/cpp  # WinHTTP 请求与 JSON 解析
  TodayTokenItem.h/cpp    # Token / Cost 显示项
  PluginInterface.h       # 插件接口
  build.bat / build64.bat # MSVC x86 / x64 编译脚本

tests/frontend/           # Vitest 测试
docs/decisions/            # 决策记录与 check.mjs
changelog-site/            # 变更日志生成与展示
.agents/skills/            # build、dev、version 项目技能
```

## 开发命令

以下命令从仓库根目录执行：

```bash
pnpm install
pnpm dev:tauri             # 前端 HMR + Rust 增量编译
pnpm dev:tauri-frontend    # 仅启动 Vite 前端
pnpm build:tauri-frontend  # TypeScript 检查 + 前端生产构建
pnpm build:tauri           # Tauri 生产构建与安装包
pnpm tauri build --no-bundle # Windows 免安装 EXE，跳过安装包
pnpm test                  # 前端测试（Vitest）
pnpm test:rust             # 自动切换到 src-tauri 后运行 cargo test
node docs/decisions/check.mjs # 决策记录校验
```

Windows 免安装产物为 `src-tauri/target/release/cc-switch-analyzer.exe`；运行中的同路径 EXE 会占用构建输出，重打包前关闭它。开发服务器已运行时复用现有实例，前端修改由 HMR 生效。

### TrafficMonitor 插件编译（仅 Windows）

需要 Visual Studio 2022 Build Tools（MSVC）。在仓库根目录的 PowerShell 中执行：

```powershell
Push-Location traffic-monitor-plugin
.\build.bat
.\build64.bat
Pop-Location
```

Git Bash 中可通过 `powershell.exe -Command` 调用上述脚本。产出自动复制到 `src-tauri/resources/`。修改 C++ 源码后必须重新编译对应 DLL，再构建并重启 Tauri 应用，因为 DLL 通过 `include_bytes!` 静态嵌入。macOS / Linux 不编译此插件。

## 关键约定

### 平台适配器与数据源契约

- 前端通过 `src/platform/index.ts` 导出的 `platformAdapter` 调用后端，不在视图里直接调用 Tauri API。
- 数据源类型与规范名由 `services/data_source.rs::DbType` 定义：`CCS`、`OpenCode`、`AIProxy`、`Cursor`、`ZCode`、`Proma`、`DSH`、`MiniMax`、`Antigravity`、`Kimi`、`PI`、`Magpie`。
- `SourceCapabilities` 声明会话管理、项目归属和增量扫描能力；用量统计可用不代表该源支持会话管理或终端恢复。
- 持久化数据源时同时保存路径和类型。扫描源可能共用 `pricing.db`，mtime 缓存键必须包含 `db_type` 与路径。

### 版本与项目文档

版本号同步修改 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`；运行 `cargo check` 同步 `src-tauri/Cargo.lock`。使用 `version` 技能时，从最近发布 tag 到 HEAD 的完整提交区间收集变更，更新 `changelog-site/data.json`。

`AGENTS.md` 与 `CLAUDE.md` 保持相同的项目约定；`README.md` 面向用户说明功能、安装与数据来源。版本升级时同步三份文档的版本说明；新增数据源、用户功能或开发命令时同步对应章节，路径和能力以当前代码为准。

### 数据库与扫描入库

- **外部 SQLite**：使用 `rusqlite::SQLITE_OPEN_READ_ONLY` 打开；包括 ZCode 和 MiniMax 的扫描输入库。
- **应用自有库**：`~/.cc-switch-analyzer/pricing.db` 可读写，保存定价、设置、任务、会话元数据以及扫描型源的派生用量。
- **扫描型源**：ZCode、Proma、DSH、MiniMax、Antigravity、Kimi、PI、Magpie 的用量写入 `session_request_logs`，增量状态保存在 `session_log_sync`；读取侧通过对应 `*_db.rs` 查询应用库。
- **Cursor**：CSV 与归因缓存按账号放在 `~/.cc-switch-analyzer/cursor-cache/<userId>/`，不改 Cursor 原始用量数据。

### 新增本地用量源

| 数据源 | 当前采集方式与注意点 |
|--------|--------------------|
| ZCode | 只读 `~/.zcode/cli/db/db.sqlite` 的用量，增量备份到应用库，已导入历史不随源库的 30 天裁剪删除 |
| Proma | 扫描 `~/.proma/agent-sessions/*.jsonl`，兼容 SDK / AgentMessage 格式，从索引补充项目归属 |
| MiniMax Code | 优先只读 `~/.minimax/v2/sqlite/runtime-state.sqlite`，无 SQLite 时回退旧版 JSONL；SQLite 行须携带 `request_duration_ms`，避免把累计上下文快照计为单次消耗 |
| Antigravity | 从 `~/.gemini/antigravity*` 会话目录发现 ID，通过正在运行的 language server RPC 读取用量；IDE 关闭时仍可查询已导入历史 |
| Kimi | 扫描 Kimi CLI / Kimi Code / Kimi Work 的 `wire.jsonl` 用量事件，并导入会话标题、项目路径 |
| PI | 合并 pi、oh-my-pi CLI 与 OMP 桌面版 profiles 的会话 JSONL，按请求去重；设置页展示全部实际扫描根 |
| Magpie | 只读 magpie 网关请求账本 `usage.jsonl`（`$XDG_CONFIG_HOME/magpie` 优先，否则 `~/.config/magpie`），按整行内容哈希 + 字节偏移增量导入；不接 `routing/`、`usage-others/` 与客户端上报（`source=reported`）记录 |

PI 默认扫描 `~/.pi/agent/sessions/`、`~/.omp/agent/sessions/` 与 `~/.omp/profiles/<profile>/agent/sessions/`。`PI_CODING_AGENT_DIR` 覆盖 pi agent 目录，`PI_CONFIG_DIR` 覆盖 OMP 配置根。Kimi 的实际候选目录见 `kimi_scanner::get_all_kimi_session_roots`。

Magpie 的配置目录按 `$XDG_CONFIG_HOME`（仅绝对路径生效）解析为 `$XDG_CONFIG_HOME/magpie`，否则回退 `~/.config/magpie`（Windows 为 `%USERPROFILE%\.config\magpie`，不是 `%APPDATA%`）；portable 模式的数据目录无法自动定位，由设置页 `magpie_data_dir` 覆盖。

### DSH 用量数据（双数据源模式）

由应用库 `settings.dsh_use_plugin` 切换，默认使用会话扫描：

- **会话扫描**（`dsh_scanner.rs`）：读取 `~/.dsh/sessions/**/session.jsonl.zstd|.jsonl`；`DshParseState` 按 step 跨行维护状态，以 `step/start`、首个 `assistant/chunk` 和 `assistant/message` 推算耗时与首字时间。
- **插件数据**（`dsh_plugin_scanner.rs`）：读取 [dsh-token-usage](https://github.com/LaoYueHanNi/dsh-token-usage) 写入的按天 JSONL，直接提取 `latencyMs` / `firstTokenLatencyMs`。目录优先使用设置中的 `dsh_plugin_data_dir`，否则按 `$DSH_HOME/token-usage` 或 `~/.dsh/token-usage` 解析。

两种来源统一为 `ParsedRow` 并通过 `scan_file_incremental` 写入应用库（规范 source 为 `DSH`）。扫描旧行时仍需解析以重建跨行状态，仅让 offset 之后的新行入库。读取侧 `dsh_db.rs` 与扫描模式无关；Rust 后台采集、手动扫描与 `source_mtime` 按当前模式及自定义目录路由。

### Cursor 用量与计价

使用 Session Token 从 API 同步 CSV，缓存 TTL 为 24 小时；支持同步窗口、多账号查询和每账号独立的本机精准归因开关。归因依据本机 Hook 日志的时间窗与模型家族，可行级改判；退出登录默认保留缓存（`cursor_logout(clear_cache=false)`）。

轻量模式下 Cursor 只展示已有 CSV 缓存，不新增后台 API 自动请求，也不缩短现有前台自动同步的 24 小时有效期。需要最新数据时由用户手动同步；Cursor 排除在本地数据源后台实时更新目标之外，避免增加云端请求频率。

Cursor CSV 行是聚合用量，不能用 `input + cache_read` 推断单次请求的上下文档位；`pricing_context_width` 对 Cursor 返回 0，使用基础价，分时规则仍有效。

### CCS 过滤与实时轮询

CCS 会话日志同步记录可按 OpenCode、Claude Code、Codex、Grok Build、MCode、Gemini CLI、Hermes 过滤，代理转发记录保留；`claude-desktop` 继续排除在会话统计及会话详情之外，但保留在实时请求列表和实时分钟趋势中。设置写入应用库，并同步 `SharedState.ccs_filter_session_apps`，主页面、TrafficMonitor 和 macOS 菜单栏共用该口径。

`DataSource::stream_records` 的秒级游标采用闭式语义：返回 `created_at >= since`，前端 `useRealtimePolling` 用指纹去重，避免漏掉同秒后到的记录。实时响应保留重映射前的 `sourceSessionId` 参与指纹，`sessionId` 的 Codex 会话归属映射变化不得导致重复追加。实时页按数据源展示，分别显示首字、耗时与输出/总速度；缺失指标不伪造。

### 会话与终端启动

`session_manager.rs` 和 `multi_terminal.rs` 在 Windows / macOS 支持启动、恢复 Claude Code、OpenCode、Codex、Grok Build，并支持任务分屏。支持某源的会话统计不等于支持该终端的恢复命令。

`codex_sessions.rs` 用本地 Codex 会话辅助真实 session 映射与标题识别；`grok_sessions.rs` 只读 `$GROK_HOME` 或 `~/.grok/{sessions,archived_sessions}/` 的元数据，不单独采集 Grok 用量。

### 定价页与 Token 输入

云端 `aliasPatterns` 经校验后以 JSON 数组保存到应用库；`cloud_pricing_cache_format=2` 标记字段已完整下载，旧格式即使云端版本相同也须补写。Rust `ModelResolver` 与前端 `createModelResolver` 共用模型 ID、历史／用户精确别名、正则兜底三阶段，忽略 ASCII 大小写并要求完整名称匹配；歧义不选候选，禁止剥离模型身份。新增模型识别消费者须复用统一解析器。

模型卡片仅在 hover 时显示“带入定价”，经 Pinia 一次性交接输入、输出、缓存读、缓存写四项原始 Token，并切换到定价页。交接请求消费后清空。

定价工具栏支持家族分组、全部模型卡片、紧凑排行榜三种展示，按费用或模型名排序，并用箭头切换升降序。筛选与四项模拟用量共用。

`CompactTokenInput` / `useTokenInput` 分开保存实际 Token 与显示文本：外部带入按量选择 K/M/B；手工输入期间保留当前单位和原始文本，失焦或 Enter 后按 `1000 K → 1 M`、`1000 M → 1 B` 进位，清空立即回到 K。不得逐键改写输入，否则后续数字会被接到换算后的小数上；费用、上下文档位与排序始终使用实际 Token 数。

### 常驻服务与后台模式

- **后端初始化与采集**：启动时由 Rust 恢复数据源、禁用状态和缓存定价；`auto_load_database` 在界面重建时复用已有状态。`background_collection.rs` 启动独立线程，每轮结束后等待 5 秒，扫描已启用的 ZCode、Proma、DSH、MiniMax、Antigravity、Kimi、PI、Magpie；前端刷新共用采集闸门，Cursor 不进入 API 同步。采集持独立应用库连接，避免长扫描占用界面设置的 Rust Mutex；SQLite 写事务仍由 WAL / busy timeout 协调。
- **启停与变更检测**：禁用状态以路径 + 类型存入 `disabled_sources`，兼容旧 `disabled_source_paths`。ZCode / MiniMax 源库变更戳覆盖主库与 WAL 的 mtime、长度及创建/删除；前端刷新不再仅凭最新秒级时间戳相等跳过变化。
- **共享今日查询**：`today_query.rs` 只做当日记录去重、Token 汇总及逐请求定价，复用前台定价引擎（含上下文档位、别名、时间规则与峰谷价），避免完整图表预计算。HTTP 与菜单栏各持独立只读 `TodaySourceCache`；SQL 源实时读库，Cursor CSV 变更时重开。八类扫描源的日期条件在 SQL 执行，应用库建立 `(source, created_at)` 索引；禁止给外部库加索引。
- **Windows TrafficMonitor**：`http_server.rs` 在 `127.0.0.1:19810–19820` 提供 `/api/today`，按请求计算，不缓存 30 秒结果；停服等待监听线程释放端口。费用由后端计算，`totalCost` 返回 `"172.57¥"` 等格式化字符串。插件在实际请求失败后每次最多探测一个端口，失败轮次有退避，成功后重置；改 C++ 后需重编 x86 / x64 DLL。`traffic-monitor-plugin/test.bat` 验证真实 WinHTTP 断线及二次恢复。
- **状态与日期**：今日响应附带 `stale`、`errors`、`updatedAt`；全部源失败返回 HTTP 503，部分失败显式标记并重试失败连接。Windows 插件展示 `stale` 部分结果中的可用数字（Antigravity 未运行等单源常态不拖垮展示），仅全部源失败（HTTP 503 / error）显示 `--`；macOS 部分结果以 `*` 标记、全部失败显示 `—`。新版插件与菜单栏按本机日历独立计算两次午夜，支持半小时时区与 23/25 小时日；HTTP 保留整小时 `tz`，新增 `tz_minutes`、`tz=local`。
- **macOS 菜单栏**：`menubar.rs` 每轮查询后等待 5 秒，`menubar_macos.rs` 用原生 `NSStatusItem` 展示两行 Token / 费用；停止及重新启动通过 generation 校验丢弃旧查询/排队回调。AppKit UI 操作在主线程执行，原生回调用 `try_state` 处理尚未注册的状态。
- **后台常驻**：`headless.rs` 在主线程协调可见、销毁中、后台、重建中四态；进入后台销毁 WebView、保留 Rust 进程与采集/状态栏服务。Destroyed 确认后才重建，快速唤起排队；外观在失败时回滚，所有入口同步托盘文案。默认关闭只隐藏。“关闭窗口时销毁界面”是配置，“立即进入后台模式”是动作。窗口保存内部尺寸与正常态几何，仅在新建时恢复，忽略旧版外部尺寸；页面回到默认路由和筛选状态。
- **验证边界**：本地采集与菜单栏各有 5 秒调度间隔，通常目标为源数据可读后约 5–10 秒加扫描/查询耗时；Windows 展示还受 TrafficMonitor 轮询周期影响。Cursor 保持缓存展示，Antigravity 依赖正在运行的 language server。修复与回归范围见 [轻量模式审计](docs/audits/2026-10-10-lightweight-mode.md)；原生窗口、内存及 macOS UI 仍需目标设备验收。
