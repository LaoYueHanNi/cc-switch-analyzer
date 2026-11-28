# DR: Antigravity 上下文压缩用量采集与本地占位符映射字典

Status: implemented

## Problem

Antigravity 触发上下文压缩／摘要（`CORTEX_STEP_TYPE_CHECKPOINT`）会产生可观的 Token 消耗，但该请求不出现在 `GetCascadeTrajectoryGeneratorMetadata` 的 `chatModel` 数组中，仅记录在 `GetCascadeTrajectory` 的 `trajectory.steps[]` 内，导致现有扫描器完全漏计了压缩消耗。

同时，`CHECKPOINT` 步骤存在两类模型字段约束：
1. 2.34 之前的旧版轨迹仅在 `metadata.retryInfos[].usage` 记录 Token，完全不含任何模型标识；
2. 2.34+ 新版轨迹虽在 `metadata.modelUsage.model`（及部分 `usage.model`）写入了模型占位符（如 `MODEL_PLACEHOLDER_M525`），但未携带真实 `responseModel` 字符串，直接入库无法命中云端定价表算费，且未来仍可能出现尚未观测到对应真实模型的新占位符。

## Decision

在 `antigravity_scanner` 与 `app_db` 中补齐压缩采集与占位符持久化映射闭环（延续 [daemon 发现决策](2026-11-22-antigravity-daemon-discovery.md) 的 RPC 采集口径）：

1. **提取 `CHECKPOINT` 用量并过滤无模型记录**：`parse_checkpoint_steps` 从 `GetCascadeTrajectory` 的 `steps[]` 提取 `CORTEX_STEP_TYPE_CHECKPOINT`，优先读取 `metadata.retryInfos[].usage`（为空时回退 `metadata.modelUsage`），若 `usage.model` 为空则回退 `metadata.modelUsage.model`。**完全没有模型占位符的记录直接丢弃不入库**。耗时按 `completedAt - startedAt` 计算，`first_token_latency = 0`，仍以 `Antigravity:<responseId>` 作为主键与 `chatModel` 共同去重。
2. **持久化本地映射字典与内置种子表**：`app_db` 在 `migrate_v18` 新增 `antigravity_model_map (placeholder PRIMARY KEY, model_id, updated_at)` 表；内置 `DEFAULT_MODEL_MAP` 种子字典在每次 `AppDbService::new` 初始化时以不覆盖方式（`INSERT OR IGNORE`）合并入表，扫描任意会话 `GetCascadeTrajectoryGeneratorMetadata` 时从 `chatModel` 实时捕获 `(model -> responseModel)` 以覆盖更新本地字典（按会话 `mtime` 升序扫描，确保最新会话映射胜出）。
3. **保留未命中占位符并支持字典更新回刷**：入库解析优先级为「当前会话 `chatModel` 映射 > 本地持久化字典 > 保留原占位符 ID」。无论在应用启动合并新版本种子字典时，还是扫描新会话学到新映射时，均调用 `backfill_antigravity_model_ids_on_conn` 将 `session_request_logs` 中 `source = 'Antigravity'` 且仍等于占位符的历史记录回刷为新模型 ID，不改动已解析为真实模型名的记录。
4. **平滑触发历史补采**：`migrate_v18` 仅清空 `session_log_sync WHERE source = 'Antigravity'` 同步游标，保留已有 `session_request_logs` 历史账本，使后续首次运行中扫描时自动补齐历史带占位符的压缩用量并建立字典。

## Alternatives considered

- **将 `MODEL_PLACEHOLDER_*` 写入全局 `ModelResolver`／云端别名**：Antigravity 占位符属于单客户端内部枚举且可能随版本变更，污染全局 `ModelResolver` 会影响其他数据源与前端筛选列表，且无法在本地从 `chatModel` 自动学习扩展；独立表 `antigravity_model_map` 边界清晰、可离线与在线双向更新。
- **仅在查询侧 JOIN `antigravity_model_map` 不改落库值**：`antigravity_db` 与单测临时库可能不含该表，且 `get_models` 下拉及跨表算费链路需多处改写；入库解析 + 字典变更时回刷 `session_request_logs` 仍能做到「先保留原 ID、字典更新后历史立即映射」，同时零侵入下游查询管道。
- **无占位符的旧版 `CHECKPOINT` 按同会话上一轮主模型猜填**：缺乏明确模型标识，跨切换模型或历史无归属记录存在错配风险；按需求明确丢弃无占位符记录以保证账本模型准确性。

## Consequences

- **换来**：完整采集新版 Antigravity 上下文压缩的 Token 与耗时，压缩消耗可按真实模型 ID 正常参与算费与统计；新占位符未命中时不丢记录，后续版本更新种子字典或任意新会话学到映射后历史记录自动回刷生效。
- **代价**：`pricing.db` 升级到 schema v18 并在首次打开运行中的 Antigravity 时触发一次全量会话重扫；极少数完全未在任何主对话出现且未录入种子表的新占位符，在字典补全前会暂时以占位符原 ID 展示且计费为 0。
