# DR: ZCode 用量增量备份到应用库，对抗源库 30 天裁剪

Status: implemented

## Problem

分析器原先只读 `~/.zcode/cli/db/db.sqlite` 的 `model_usage`。ZCode 在每次写入用量后执行 `DELETE FROM model_usage/turn_usage/tool_usage WHERE started_at < now-30d`。超过 30 天的 ZCode 用量会从模型/供应商/趋势统计中消失，且无法找回。会话 Tab 本来就不展示 ZCode，不受影响。

## Decision

ZCode 改为与 DSH/Proma/MiniMax 相同的扫描入库：只读源 sqlite，把分析器需要的字段写入 `pricing.db::session_request_logs`（`source='ZCode'`，主键 `ZCode:{model_usage.id}`）。查询侧 `ZCodeDbService` 只读应用库，不再直连源库。入库字段限于 request_id、session_id、model、fresh input、output、cache_read、cache_creation、created_at（秒）、latency；不拷会话正文、turn、tool、message。input 在入库时把 cache-inclusive 口径减掉 cache_read。增量用源文件 mtime + 已导入的最大 `started_at`；只追加、不随源库删除。启动与刷新时扫描。

## Alternatives considered

- **继续直连源库、另建完整 sqlite 镜像** —— 能抗裁剪，但复制了用不上的 message/session 表，体积大且两套查询并存。
- **只在本机定时复制整个 db.sqlite** —— 下一次 ZCode 裁剪后镜像也被覆盖，挡不住 30 天窗口。
- **把 ZCode 改成会话扫描 JSONL** —— 源数据就在 sqlite 的 model_usage，不必绕路。

## Consequences

- 换来：ZCode 用量在源库被裁掉后仍能按月/按年出现在模型、供应商、趋势、实时统计里；字段与现有扫描源对齐，不新增表。
- 代价：应用库会随 ZCode 使用持续增长（仅精简行）；必须先扫描再查询，源 sqlite 与应用库有短暂滞后；旧版 `last_db_paths` 里指向源 sqlite 的 ZCode 条目会在加载时改打开 pricing.db。
