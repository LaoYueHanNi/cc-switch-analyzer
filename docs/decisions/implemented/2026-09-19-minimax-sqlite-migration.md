# DR: MiniMax 数据源升级为直读 SQLite 与原子重刷迁移

Status: implemented

## Problem

MiniMax Code（Mavis 桌面端）原先作为扫描型数据源接入分析器时，通过遍历 `~/.minimax/v2/sessions/.../messages.jsonl` 文件读取用量。然而该日志流格式中源头仅记录 Token 数和时间戳，完全缺失请求耗时（latency）与思考耗时（first_token_latency），导致系统在实时记录与统计视图中只能将 MiniMax 耗时置为 `0`（前端显示 `-`），且无法计算生成速率（`tok/s`）。

近期 MiniMax Code 最新版本（2026-09-18 更新）在前端界面支持了输出速度（`token/s`）显示。经实勘发现，客户端运行时将包含 `request_duration_ms` 与 `thinking_duration_ms` 的完整结构化指标写入了本地 SQLite 数据库（`~/.minimax/v2/sqlite/runtime-state.sqlite` 的 `local_runtime_message_rows` 表）。然而如果直接读取该库，由于旧实现的主键格式为 `MiniMax:msg-...`（依赖 JSONL 内部消息 ID），而 SQLite 表的消息 ID 为 UUID，主键格式不兼容会导致新旧两份数据并存造成严重的数据量翻倍重复。

## Decision

将 MiniMax 数据源的采集核心由旧的深层目录多文件 JSONL 扫描升级为直读官方本地 SQLite 数据库，并采用原子重刷与单文件游标架构：

1. **版本升级迁移（`app_db::migrate_v15`）**：在数据库升至 v15 时，原子执行 `DELETE FROM session_request_logs WHERE source = 'MiniMax'` 与 `DELETE FROM session_log_sync WHERE source = 'MiniMax'`。将原先作为派生缓存的旧格式 MiniMax 记录彻底清空，从源头上切断新旧主键不兼容导致的重复风险。
2. **直读 SQLite 与全量重灌（`minimax_scanner.rs`）**：启动与扫描时以只读连接打开 `~/.minimax/v2/sqlite/runtime-state.sqlite`，从 `local_runtime_message_rows` 表中提取全部 assistant 消息。主键统一为 `MiniMax:{msg_id}`，准确映射 `request_duration_ms` 到 `latency`，`thinking_duration_ms` 到 `first_token_latency`。模型名采用多级解析机制（`context_usage_telemetry.model` -> `data.model` -> 关联 `local_runtime_sessions.extra_data_json.effectiveModel` -> 默认回退 `MiniMax-M3`），并自动剥离 provider 前缀，彻底避免早期历史记录因缺失遥测字段而回退到未定价裸名 `MiniMax` 导致展示为缺少定价的问题。瞬时重灌历史数据，确保历史记录一条不漏，并补全所有可用请求的耗时与 `tok/s` 速度。
3. **单文件增量游标（对齐 ZCode 模式）**：增量同步改为仅监视 `runtime-state.sqlite` 的单个文件 mtime，并将已导入的最大自增 `id` 记录在 `session_log_sync.last_line_offset`。后续刷新只需过滤 `WHERE id > cursor_id`，摆脱全盘递归扫描。
4. **极端兼容降级（Fallback）**：保留针对无 SQLite 场景回退扫描 `messages.jsonl` 的兼容能力。

## Alternatives considered

- **双键映射原地 UPDATE 迁移** —— 尝试遍历旧库中的 `MiniMax:msg-...`，通过 `(session_id, turn_id, output_tokens)` 模糊匹配 SQLite 记录并更新主键和耗时字段。之所以放弃，是因为旧版 JSONL 消息有近百条为同一个 turn 下的多次工具交互，时间戳存在轻微漂移，多条件模糊关联存在错位风险，且迁移代码逻辑极其脆弱繁重，不如利用派生库特性做原子重刷来得干净可靠。
- **双轨并存追加模式（只追加新数据，旧数据保持不动）** —— 旧的 308 条保留，只从新时间点追加新 SQLite 记录。之所以放弃，是因为用户 9/18 升级后已有大量带耗时的真实记录已经先被旧扫描器录入了为 0 的记录，双轨并存无法修复这批历史数据的耗时，且在时间边界处容易出现交叉遗漏。

## Consequences

- 代价：升级至 v15 时，旧的 MiniMax 缓存记录会被清空一次，并在应用启动扫描时重新落库（耗时 < 15ms，用户无感）。
- 换来：彻底杜绝数据重复与遗漏风险；MiniMax 数据源全面点亮毫秒级请求耗时、思考耗时与 `tok/s` 速度；增量扫描从扫描数十个分散目录收敛为监视单个 SQLite 文件，启动与刷新 I/O 显著降低。
