# DR: 启动非阻塞 Cursor 自动同步、启动初始化去重与 Antigravity 非会话目录过滤

Status: implemented

## Problem

应用冷启动及后续前台轮询存在四类叠加卡顿：

1. **Cursor 同步阻塞查询链路**：`query_precompute`、`refresh_database`、`query_realtime_logs` 等入口每次都同步执行 `sync_and_reload_if_needed`。当 Cursor 已登录且 `usage.csv` 超过 24 小时 TTL，而网络请求失败（超时 8 秒或分块协议错误）时，本地文件 mtime 不会更新，且无失败退避机制，导致每次首屏查询与自动刷新同步卡住 1.4–8.0 秒。
2. **启动初始化重复计算**：`.setup()` 主线程同步调用 `initialize_sources` 计算 `to_info()`（11 个数据源逐一 `COUNT(*)`）后直接丢弃返回值；`auto_load_paths` 内部再次重复调用 `refresh_pricing`；前端 `get_filter_options` 与 `get_all_pricing` 串行逐源查询模型列表。
3. **Antigravity 扫描误命中非会话目录**：`~/.gemini/antigravity/brain/tempmediaStorage` 等非 UUID 目录被纳入候选会话，每 5 秒轮询触发 RPC 404，返回 `Ok(errors: 1)` 而未走 [Antigravity 不可达退避](./2026-10-25-antigravity-unreachable-backoff.md)，持续空转并刷屏警告日志。
4. **OpenCode 元数据多次全表扫描**：启动期在 [OpenCode 内容戳缓存](./2026-10-25-opencode-content-stamp-cache.md) 之外多次独立执行 `COUNT` / `MIN/MAX` / `DISTINCT`（详另文同步更新）。

## Decision

1. **Cursor 自动同步改为后台异步 + 失败指数退避**：
   - `sync_and_reload_if_needed` 仅同步完成本地已缓存账号的注册检查（mtime 未变时跳过）。
   - `cursor_sync` 增加模块级 `AUTO_SYNC_STATE`（`in_flight` 防重入 + `failures` + `next_retry_at`）；仅在已登录、24h 缓存过期、无进行中任务且不在退避窗口时，在 `cursor-auto-sync` 后台线程执行 `sync_cursor_cache()`，成功后再调用 `ensure_all_cursor_sources_registered_in` 与 `reload_cursor_sources_in` 刷新内存状态；失败按 `60s → 120s → 240s → 480s → 600s`（封顶 10 分钟）退避。
   - 手动触发的 `cursor_sync` / `cursor_login` / `cursor_logout` 仍同步返回结果，并在成功或登出时调用 `reset_auto_sync_backoff()` 清零退避。Cursor 仍不进入 `BackgroundCollector`。
2. **启动初始化去重与并行化**：
   - `.setup()` 改调 `ensure_sources_initialized`（仅初始化定价引擎与恢复数据源，不触发 `to_info()`）；`auto_load_paths` 移除多余的第二次 `refresh_pricing`，直接通过 `save_paths_from_entries` 持久化路径。
   - `initialize_sources`、`get_filter_options` 与 `get_all_pricing` 均使用 `std::thread::scope` 并行查询各数据源的统计/模型/日期范围。
3. **Antigravity 仅扫描标准 UUID 会话目录**：
   - `scan_session_candidates` 枚举 `brain/` 子目录时通过 `is_valid_cascade_id`（36 字符 `8-4-4-4-12` 十六进制 UUID）跳过 `tempmediaStorage` 等非会话辅助目录。

## Alternatives considered

- **把 Cursor 自动同步并入 `BackgroundCollector`（5 秒循环）** —— 违反项目约定（Cursor 不进入轻量模式后台高频采集），且无意义增加进程空闲唤醒。
- **Cursor 自动同步失败后改写 `usage.csv` mtime 充当退避** —— 伪造缓存修改时间会污染用户可见的"上次同步时间"状态，改为内存态 `AUTO_SYNC_STATE` 退避更诚实且可被手动操作立即重置。
- **Antigravity 遇到 HTTP 404 时写入 `session_log_sync` 永久跳过** —— 会把无效非会话目录写入应用库同步表；在目录枚举入口用 UUID 格式直接过滤更干净，不污染数据库。

## Consequences

- 换来：首屏与自动刷新不再受 Cursor 网络异常同步阻塞，失败时有界退避；后端 `.setup()` 避免重复定价刷新与废弃的 11 源计数；Antigravity 不再因 `tempmediaStorage` 每 5 秒触发无效 RPC。
- 代价：Cursor 缓存过期时首屏先展示已有本地缓存，后台同步成功后在下一次查询/刷新呈现新增数据；`is_valid_cascade_id` 假设 Antigravity `brain/` 下的真实会话目录均以标准 UUID 命名（非 UUID 的 `.pb` 仍由 `conversations/` 分支覆盖）。
