# DR: 模型维度展示与算费按定价表归一化（查询侧折叠）

Status: implemented

## Problem

DSH 从 magpie 网关调用模型后，会话日志 `assistant/message` 的 `source.model` 记录的是路由名（`auto/GLM-5.3`、`group/glm-5.3-flashx`、`zcode/GLM-5.3-Flash-Trial`），真实模型 id 只存在于 magpie 自己的账本（`served` 字段）。按原始名落库与分组导致 ByModel 页同一真实模型分裂成多个卡片（`auto/GLM-5.3`、`GLM-5.3`、`glm-5.3` 各一条），且算费未命中：`model_costs` 等费用表以 canonical id 为键，原始名卡片查不到费用显示 0；`resolve_model_id` 对带前缀路由名解析失败，今日统计同样漏价。

用户自己的 dsh-token-usage 插件已验证了正确的处理位置：记录保留原始名，展示与算费时按定价表归一化（`providerSuffix` 剥路由前缀 + 定价表 modelId/别名/正则三阶段匹配），byModel 行按 canonical id 折叠。

## Decision

查询展示层归一化，落库不动：

- `PricingEngine::resolve_model_id` 增加路由前缀兜底：完整名（modelId → 别名 → 正则三阶段）未命中且无歧义时，取 `provider/` 形式名的最后一段（`provider_suffix`，与插件 `providerSuffix` 同口径：多于一段且每段非空）再解析一次。完整名命中的斜杠别名（`meta/muse-glimmer-30b`、`openrouter/free`）优先，不受剥离影响；歧义仍返回 None 不猜。定价表来自云端 model-price-table（应用库 `aliasPatterns`），与插件共用同一数据源。
- `query_by_model` 聚合后按 canonical id 折叠（`fold_model_breakdown`）：同名行 requests 与四类 Token 求和，解析不出的行保留原始名，按总量降序。
- `query_daily_trend` / `query_hourly_trend` 聚合后规范化并按 (day, model) 折叠（`fold_trend_rows`），平均耗时按请求数加权合并；补齐了此前只规范 hourly 不规范 daily 的不一致。
- 作用范围仅模型维度展示聚合（ByModel、趋势、预计算费用的 canonical 键）。落库、会话明细（SessionAnalysis 卡片）、实时请求列表保留原始名；模型筛选下拉仍列各源原始名、按精确名过滤。

## Alternatives considered

- **落库时归一化（扫描入库改写 model + 历史迁移）** —— 违背「各源保留完整原始账本」的约定，且需要一次性迁移；用户明确以插件为参照（插件记录层保留原名），展示层折叠零迁移、可随定价表更新重算。
- **跨源按特征匹配 magpie `served` 改写 DSH 模型名** —— 真实模型只记录在 magpie 账本，按（时间 + 四类 Token）跨源关联脆弱，且 DSH 与 magpie 是两套独立账本，配对失败的行仍无解；定价表归一化不依赖跨源配对，覆盖所有历史与未来数据。
- **把前缀剥离内置进共享 `ModelResolver`** —— 改变共享原语契约（约定「要求完整名称匹配、禁止剥离模型身份」），影响云端定价加载、写入规范化等全部消费者；兜底放在 `PricingEngine::resolve_model_id` 这一个定价/查询消费者入口，语义边界清晰。

## Consequences

- 换来：ByModel 与趋势页同一真实模型单卡片显示（`auto/GLM-5.3`、`GLM-5.3`、`glm-5.3` 合并），费用键对齐不再显示 0；今日统计、菜单栏、HTTP 的算费对路由名生效；历史数据零迁移即被覆盖；定价表更新后重新查询即按新表归并。
- 代价：模型筛选下拉与折叠后卡片名不一致（下拉列原始名，筛选按精确原始名匹配，选 canonical 名过滤不到原始行）；未定价的路由别名（如 `GLM-5.3-Flash-Trial`，定价表无此 id）保留原名单列且不计费；会话明细仍显示原始名，与 ByModel 卡片名不同。
- 调试与验证：`provider_suffix` 与解析兜底（斜杠别名优先、空段拒绝、未定价 None）由 `model_resolver.rs` / `pricing_engine.rs` 单元测试覆盖；ByModel 与趋势折叠（canonical 合并、Trial 保留、加权耗时）由 `commands/query.rs` 单元测试覆盖。
