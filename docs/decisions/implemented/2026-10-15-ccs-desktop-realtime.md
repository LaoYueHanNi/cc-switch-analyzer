# DR: CCS 桌面端记录保留在实时数据、排除会话统计

Status: implemented

## Problem

CC-Switch 的 `proxy_request_logs` 中存在 `app_type='claude-desktop'` 的真实请求。此前的实现提交 `d01d336` 将它从会话统计、实时请求列表和实时分钟趋势全部排除，并在通用查询中清空桌面端逐请求的 `session_id` 以跳过会话聚合；该提交没有配套决策记录解释实时侧排除的理由。趋势分析页的按天、按小时趋势仍保留桌面端请求，实时排查却看不到这些数据。

本决策取代 [CCS 会话同步终端过滤记录](2026-09-27-ccs-session-filter-full-app-types.md) 中关于 `claude-desktop` 全局排除的描述，保留该记录其余终端过滤决定。

## Decision

CCS 的 `claude-desktop` 记录按用途分流：

- `get_session_breakdown` 继续排除桌面端，所有会话模型 Token、会话请求 Token、会话上下文和时间戳辅助查询补齐 `app_type != 'claude-desktop'`；通用查询仍清空桌面端逐请求的 `session_id`，会话聚合因此跳过它。
- `get_recent_request_logs_raw` / `stream_records` 保留桌面端记录，因此实时请求列表可展示它们；已有 CCS 会话过滤设置仍只排除匹配的非 `proxy` 会话同步记录，不额外排除桌面端代理请求。
- 后端 `get_minute_level_token_trend` 保留桌面端记录。当前 `RealtimeToken.vue` 仅展示请求列表，通过 `query_realtime_logs` 读取，不调用此分桶接口；趋势分析页的按天、按小时曲线继续通过通用趋势查询包含桌面端记录。

## Alternatives considered

- **继续全局排除桌面端** —— 输。会话碎片问题可以在会话查询侧单独处理，但全局排除会隐藏用户排查实时请求所需的数据。
- **把桌面端加入会话过滤设置** —— 输。桌面端的实时代理请求也应可见，配置项无法表达“会话统计排除、实时数据保留”的固定口径，且会让用户误以为可以改变该约束。
- **只恢复实时请求列表，不恢复实时分钟趋势** —— 输。实时趋势和实时列表都服务于当前请求排查，单独恢复会造成两者继续不一致。

## Consequences

- 换来：实时请求列表、实时分钟趋势、趋势分析按天／按小时曲线都能看到 CCS 的 Claude Desktop 请求；会话页不再产生桌面端碎片会话。
- 代价：同一 CCS 数据源在会话统计与实时数据中的桌面端口径不同，查询实现和测试必须明确维护这条边界。
