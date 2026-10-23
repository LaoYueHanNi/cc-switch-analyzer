# DR: 接入 Magpie 网关请求账本作为本地用量数据源

Status: implemented

## Problem

用户在本机运行 magpie 网关，把 agent 的模型请求统一转发到上游供应商；magpie 把每次真实网关调用追加写入配置目录下的请求账本 `usage.jsonl`，一行一次调用，带时间、模型、四类 Token、耗时、首字与状态。在此之前，应用只覆盖 CC-Switch、OpenCode、AI-Proxy 等代理／客户端与各终端会话日志，Magpie 用户的本机网关用量完全不可见，只能去 magpie 自己的界面查看。

接入需要满足几个约束：不修改 magpie 的任何文件，只读它的账本；magpie 未运行时也能查询已导入历史；只统计真正经过本机的网关调用，不把其他电脑同步来的账本、客户端上报的调用或 magpie 自己从 agent 会话日志扫描出的用量混进来，否则会跨机重复计费，或把未经网关的调用当成网关用量。

## Decision

新增数据源 `DbType::Magpie`，规范名、`session_request_logs.source` 与供应商 id 统一为 `Magpie`。扫描侧为 `services/magpie_scanner.rs`，只读查询侧为 `services/magpie_db.rs`；`services/mod.rs`、`background_collection.rs`、`commands/database.rs`、`lib.rs` 完成接入。

**数据位置与注册**。账本路径按 magpie 的 `internal/appdir` 规则解析：`$XDG_CONFIG_HOME` 为绝对路径时取 `$XDG_CONFIG_HOME/magpie/usage.jsonl`，否则取 `~/.config/magpie/usage.jsonl`（Windows 同样在 `%USERPROFILE%\.config\magpie`，不是 `%APPDATA%`）。portable 模式的账本在 magpie 可执行文件旁，应用无法自动定位可执行文件，由设置页自定义目录覆盖（settings 键 `magpie_data_dir`，命令 `magpie_settings` / `set_magpie_data_dir`，空串恢复默认）。自定义目录优先于默认解析；配置目录存在即注册数据源，账本尚未创建时扫描返回 0 条，用户首次请求后下一轮自动导入，无需重启应用。

**记录映射**。`in` → `input_tokens`、`out` → `output_tokens`（magpie 的 out 已含 reasoning token）、`cache_read` → `cache_read`、`cache_write` → `cache_creation`、`t`（RFC3339Nano）→ `created_at`（Unix 秒）、`ms` → `latency`、`ttft_ms` → `first_token_latency`、`model` → `model`。`model` 缺失时依次回退 `served`、`req`，仍为空则记 `unknown`。记录里的 `session` 用作会话 id，参与实时列表与跨源去重指纹；没有会话头时用 `magpie:<request_id>` 兜底，避免同模型同 token 的不同请求被指纹合并。

**过滤口径**。只接入网关账本本身：不读 `routing/`（路由调试轨迹）、`usage-others/`（其他电脑同步来的账本），跳过 `source = "reported"`（客户端 HTTP 上报、实际未经网关）与显式 `rejected`、四维 token 全 0 的本地拒绝／无计费失败记录。`handed_on` 交接行只要携带 token 就保留，其 token 与 magpie 汇总一致；magpie 的 `Calls` 不计交接行，本应用请求数按记录数统计，因此会略高于 magpie 的 `Calls`。

**去重与增量**。`request_id = "Magpie:" + 整行内容（去掉行尾换行与首尾 ASCII 空白）的 FNV-1a 64 位稳定哈希 + 行字节长度`，写入 `session_request_logs` 主键，文件被重写、游标丢失或重复扫描都由主键幂等兜底。刻意不用标准库 `DefaultHasher`，其算法跨工具链版本不承诺稳定，升级后会改变去重键。增量状态存 `session_log_sync`，以文件 mtime + 已读字节偏移推进；该表的 `last_line_offset` 在本源明确表示字节偏移而非行号。尾部尚未写完换行的半行不解析、不推进游标，留待下一轮；文件变短（截断或替换）时重置游标从头读，再由主键去重避免双计。

**能力与展示**。读取侧只查应用库 `source = 'Magpie'` 的记录；`capabilities` 仅开 `incremental_scan = true`，不做会话管理、项目归属或会话标题（`get_session_*` 返回空，不写 `sessions` / `session_titles`）。供应商维度统一显示 `Magpie`，不暴露 magpie 内部路由到的上游 provider id。速度口径复用既有 `latency`、`first_token_latency` 与 `output_tokens` 计算，不扩表保存 magpie 原生 `flow_ms`。实时页沿用闭式秒级游标与指纹去重。

**采集与今日统计**。`background_collection.rs` 每轮的扫描源增加 Magpie，本地扫描源变为 ZCode、Proma、DSH、MiniMax、Antigravity、Kimi、PI、Magpie 八类；`scan_magpie_now` 供手动扫描。今日查询按已注册数据源读取，Windows TrafficMonitor 与 macOS 菜单栏的今日统计自动覆盖新源。magpie 的 `usage.jsonl` 只读，写入仅落在应用自有 `pricing.db`；已导入历史不随 magpie 账本轮转或删除而消失。后台采集与今日统计的分工见 [Rust 后台采集与状态栏及时更新](2026-10-10-background-local-collection.md)。

## Alternatives considered

- **改用 magpie HTTP API 拉取用量** —— 需要 magpie 进程存活、探测端口并处理鉴权与请求失败，多一层失败面；`usage.jsonl` 在磁盘上始终可读，离线也能补历史，口径也与 magpie 自己的汇总一致。
- **同时接入 `usage-others/` 与 `source=reported`** —— 用户明确只要本机网关账本：`usage-others/` 是多机同步，跨机器会重复计费；`reported` 是客户端上报、实际未经网关的调用，混入后“网关调用”口径不再成立。
- **保留 magpie 原生 `flow_ms` 作为速度指标** —— 需要给 `session_request_logs` 扩列，并让读取侧、今日查询、实时页与前端各增加一条速度口径；现有 `latency` + `first_token_latency` + `output_tokens` 已能算耗时、首字与输出速度，跨源一致比多一个原生字段更重要。
- **也接入 magpie 从 agent 会话日志扫描出的用量** —— 用户明确不要；那部分与 Kimi、DSH 等本地会话日志来源重叠，且不是网关调用，会把同一请求按两种口径重复计入。
- **按 magpie 内部上游 provider 拆分供应商** —— 用户选择统一显示 `Magpie`；上游 provider 是网关内部路由概念，拆分后与应用其他数据源的供应商维度不一致，也会把用户不想维护的内部拓扑暴露到筛选与图表里。

## Consequences

- 换来：magpie 网关调用进入统一的 Token、费用、模型分布、趋势、实时列表与今日状态栏统计；只读磁盘账本，magpie 未运行也能查询已导入历史；mtime + 字节偏移让未变化文件零成本跳过，内容哈希主键让重扫与文件重写幂等；四类 Token 与 magpie 汇总口径一致；供应商维度干净，不影响其他数据源。
- 代价：请求数按记录数统计，`handed_on` 交接行会让 Magpie 的请求数略高于 magpie 自身的 `Calls`；不接路由轨迹、多机账本与客户端上报，因此应用里无法还原 magpie 的路由决策或多机汇总，也不统计未经网关的调用；统一供应商后无法按上游 provider 拆分费用与缓存表现；portable 模式必须手动指定目录，否则发现不了数据；账本被手工截断、替换或删除时（magpie 自身只追加、不轮转）已导入历史仍保留，但应用无法感知源端撤回；四维 token 全 0 的本地拒绝请求不计入；没有会话管理与项目归属，会话页不出现 Magpie；速度口径不含 `flow_ms`，与 magpie 自带统计的吞吐数字可能不同。
- 调试与验证：解析、过滤、去重、增量、半行与截断行为由 `magpie_scanner.rs` 单元测试覆盖；读取侧按 `source` 隔离、会话能力关闭由 `magpie_db.rs` 单元测试覆盖；外部账本只读的约束与其他数据源一致。
