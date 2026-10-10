# DR: TrafficMonitor 插件部分失败仍展示可用数字

Status: implemented

## Problem

0.8.9 的插件在响应包含 `"stale":true` 时按失败处理、显示 `--`，而 `stale` 只要有任一已启用源的后台采集报错就为真。Antigravity 的 language server 不运行是环境常态（IDE 关闭后仍可查询已导入历史），于是已启用 Antigravity 的机器上插件从后台第一轮采集完成后永久显示 `--`——仅在应用启动后首几秒（`collection_errors` 尚为空）能短暂拿到数据。实测接口持续返回完整今日汇总（如 220 请求、60.58¥），数据本身没有丢失。

用户裁决的规则：查不到不算错误，所有渠道都失败才算错误。

## Decision

插件的失败判定只保留响应含 `"error"`（全部源失败时 HTTP 503 的错误体）；`"stale":true` 的部分结果照常解析并显示 `totalTokens` / `totalCost`。服务端不变：任一源可读即返回 200 + 数据，`stale`/`errors` 仍如实标记（macOS 菜单栏的 `*` 标记行为不变）。真实 WinHTTP 回归测试新增第三段：`stale:true` 响应必须显示数字而非 `--`。

## Alternatives considered

**服务端把「language server 未运行」排除出 stale**：只修好 Antigravity 一种常态；其他单源常态失败（目录被移走、文件损坏）仍会拖垮展示，且要在错误分类里维护白名单。

**stale 只反映本次查询读取失败、采集错误不算**：语义更细，但弱化采集故障提示，macOS `*` 标记随之变少；也与「已入库历史仍可查」的现实不符——查询成功不代表数据新鲜。

**DLL 显示数字并附 `*` 标记部分结果**：与 macOS 对齐，但 TrafficMonitor 显示项空间有限，且用户明确选择「查不到不算错误」，先保持最简展示。

## Consequences

- 代价：单源失败时 Windows 插件显示的今日数字可能偏小且无标记，用户需自行查看应用内 `errors` 才知道缺了哪个源；旧版 DLL（0.8.9 随包）仍会对 stale 显示 `--`，需替换为新构建。
- 换来：Antigravity 等环境常态不再让插件永久失效，`--` 只在全部源失败（HTTP 503）时出现，与「所有渠道都失败才算错误」的裁决一致；服务端与 macOS 行为零改动。

本记录部分取代[后台采集与状态栏更新](2026-10-10-background-local-collection.md)中「不完整统计显示 `--`」的插件判定；其余决策维持。
