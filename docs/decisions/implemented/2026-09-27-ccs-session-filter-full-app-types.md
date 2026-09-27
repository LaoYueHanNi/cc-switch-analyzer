# DR: CCS 会话同步过滤补全至 cc-switch 全部终端类型

Status: implemented

## Problem

CCS 数据源的「过滤会话日志同步写入记录」只在 `CCS_SESSION_APPS`（`src/components/layout/settings/SettingsDataSources.vue`）列出 4 种终端：opencode / claude / codex / grokbuild，与本应用 `multi_terminal` 支持的启动终端一一对应。但 cc-switch 上游同步写入会话用量的终端不止这 4 种：实测本机 `~/.cc-switch/cc-switch.db` 的 `proxy_request_logs` 中，`app_type='mcode'`（`data_source='mcode_session'`）已有 475 条会话同步记录，UI 无选项、无法排除；`providers` 表还出现 `gemini`、`hermes`，将来同样会产生会话同步记录。开启过滤的用户若同时使用 MCode，统计仍被会话日志重复计入。

## Decision

`CCS_SESSION_APPS` 补全为 7 项，新增 `mcode`（MCode）、`gemini`（Gemini CLI）、`hermes`（Hermes）。后端 `set_ccs_session_filter`（`src-tauri/src/commands/database.rs`）对 apps 无白名单校验、SQL 按参数绑定，过滤子句按值匹配 `app_type IN (...)`，因此仅需改前端选项列表。`claude-desktop` 不设选项：查询侧已全局排除（`app_type != 'claude-desktop'`），加了也是死开关。

## Alternatives considered

- **只加 mcode，等 gemini/hermes 有数据再补** —— 输。`providers` 表证明 cc-switch 已支持这两种类型，等到有数据再补等于再漏一次；一次补齐的代码成本与只加一项相同。
- **后端加 app_type 白名单校验** —— 输。校验列表会成为前端选项的第二份拷贝，两处漂移的风险大于收益；未知值命中不到 `IN` 子句自然不生效，且参数绑定下无注入面。
- **沿用 4 项、把 mcode 等记入文档提示用户** —— 输。过滤能力的缺口在 UI 上不可见，用户只会看到「勾了过滤但数字还是对不上」，文档提示解决不了。

## Consequences

- 换来：勾选 MCode 后 475 条既有会话同步记录可被排除，与其他终端行为一致；gemini/hermes 将来产生数据时无需改代码即可过滤。
- 代价：cc-switch 未来新增会话同步终端类型时仍需手动补选项——本决策未建立选项与上游类型清单的同步机制；gemini/hermes 当前无数据，对应选项短期勾选无效果（也无副作用）。
