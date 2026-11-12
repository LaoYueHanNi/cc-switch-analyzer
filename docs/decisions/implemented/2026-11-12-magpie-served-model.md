# DR: Magpie 模型名取供应商实际应答模型（served 优先）

Status: implemented

## Problem

magpie 账本一行里带三个模型字段，语义各不相同（magpie 源码 `internal/usage/usage.go`）：`req` 是客户端请求的模型名（magpie 别名、路由组、`provider/model`），`model` 是网关实际发给供应商 API 的模型 id，`served` 是供应商**响应中声明**实际应答的模型（"the model the vendor's reply says answered"，供应商命名时才有）。接入时的[原始口径](2026-10-23-magpie-gateway-usage.md)取 `model` 优先、`served`、`req` 兜底。

用真实账本（约 1.4 万行）核对，`model` 与"实际模型"有三种系统性偏差：

- **供应商内部代号**：qoder-cn 收到 `dfmodel`、实际由 `DeepSeek-Flash` 应答——落库为无意义代号，既进不了模型分布，也匹配不到定价；
- **带路由前缀**：`zhipu/glm-5.3` 这类发送名带 `provider/` 前缀，而 `served` 是干净的 `glm-5.3`；
- **供应商换模型应答**：发送 `grok-4.7`，供应商实际用 `grok-4.7-build` 构建应答。

用户期望统计一律取实际模型。三种偏差中只有前缀一种能靠查询侧剥前缀兜底救回，代号与换模应答在 `model` 口径下无法恢复。

## Decision

`magpie_scanner.rs` 解析行的模型优先级改为 **`served` → `model` → `req`**：优先供应商响应声明的实际应答模型；`served` 缺失（旧版记录、供应商响应不命名模型，实测约 7% 行）回退 `model`（网关发送 id），最后 `req`，仍为空记 `unknown`。空串 `served` 视同缺失。四类 Token、去重键（整行内容哈希）、增量游标与过滤口径全部不变——`served` 只改写 `session_request_logs.model` 一列。

**历史重刷**：Magpie 走 mtime + 字节偏移增量扫描，改解析逻辑不会重扫已入库行，历史必须显式迁移。settings 新增键 `magpie_parse_version`（当前 `2`）：`scan_magpie` 入口检查该键，与当前版本不一致（含首次使用与旧版升级）即先 `clear_session_logs("Magpie")` 删除本源全部行与游标，再按新口径全量重扫；清空失败不写版本，下一轮自动重试；重扫由 request_id 主键幂等兜底，重复触发无双计。后台采集与手动扫描共用该入口，对所有用户一次性生效，无需 UI 操作。

## Alternatives considered

- **保持 `model` 优先，靠查询侧剥前缀兜底** —— 只能救回 `provider/` 前缀这一种偏差；`dfmodel` 代号与 `grok-4.7-build` 换模应答在发送名里不存在，救不回；且落库明细、模型筛选下拉、原始名过滤仍是旧名。
- **只对新数据生效，历史行不动** —— 同一模型分裂成两套口径（`dfmodel` 与 `DeepSeek-Flash` 各算一行），模型分布与费用比单一旧口径更失真。
- **加设置页"重新扫描"按钮手动重刷** —— 多一条命令与交互面，还要用户自己知道该按；版本迁移自动完成，万级行重扫瞬时结束。
- **改取 `req`** —— 那是客户端请求视角（别名、路由组），与实际计费模型无关，接入时即已排除。

## Consequences

- 换来：模型分布、趋势、费用、实时列表按实际应答模型统计；`dfmodel` 类代号不再出现，`DeepSeek-Flash` 能匹配到定价；带前缀发送名落库即为干净模型名，不再依赖查询侧剥前缀兜底。
- 代价：`served` 缺失的行（约 7%：旧版记录、响应不命名模型的供应商）仍回退 `model`/`req`，口径是发送 id；升级后首轮扫描先清空 Magpie 已入库行再同步重扫，该轮内历史行短暂重建（万级行瞬时完成）；与 magpie 自身界面若按发送模型汇总的数字在模型维度不再逐行对齐（Token、请求数总量不变）。
- 调试与验证：`served` 优先、空 `served` 回退由 `served_model_wins_over_sent_and_requested` 覆盖；版本迁移清空重扫与幂等由 `parse_version_change_rescans_history` 覆盖；真实账本冒烟沿用 `real_usage_file_smoke`。
