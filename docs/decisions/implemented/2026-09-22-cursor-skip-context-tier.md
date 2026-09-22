# DR: Cursor 数据源不参与上下文档位计价

Status: implemented

## Problem

上下文档位用单条记录的 `input + cache_read` 当作这次请求的上下文宽度，命中阈值后改用该档单价。
Cursor 用量把一段时间内的 token 聚成一条记录，这条和不是单次上下文。

Grok 4.7 增加上下文定价（256K 档）之后，Cursor 里很多聚合记录被当成超大上下文，按高档价计费，费用虚高。
其他数据源按单次请求记账，同一套规则仍然有效。

## Decision

`pricing_context_width`（`src-tauri/src/services/data_source.rs`）在 `db_type` 或 `provider_id` 为 `Cursor` 时返回 0，否则仍为 `input + cache_read`。

宽度 0 不会命中任何正阈值档位，Cursor 记录只走基础价。基础价节点上的峰谷时间规则仍然生效。
调用点：

- `aggregate_model_context_tier_buckets`：总览、按模型费用、对比分桶
- `compute_session_costs` / `compute_session_model_costs`：按 `provider_id` 判断
- 实时请求日志：按解析出的数据源名与 `provider_id` 判断，不再标出上下文档位

## Alternatives considered

- **只让 Cursor 的 `get_model_context_tier_buckets` 返回空** —— 主路径 `compute_precompute` 不走各数据源的这个方法，而是对去重后的全部记录调用 `aggregate_model_context_tier_buckets`。空桶还会在存在其他档位时把 Cursor 的 token 从替换后的模型费用里丢掉。
- **在 `SourceCapabilities` 上加「支持上下文档位」** —— 计价发生在数据源混流之后的 `RawRecord` 上，能力声明到不了记录。记录上已有 `db_type` / `provider_id`，直接判断即可。
- **在 `RawRecord` 上加布尔字段** —— 每个构造点都要改，而 Cursor 记录的 `db_type` 与 `provider_id` 已经都是 `Cursor`。

## Consequences

- 换来：Cursor 上带上下文档位的模型（当前是 Grok 4.7 的 256K 档）按基础价计费，不再因聚合 token 被抬到高档。
- 代价：Cursor 里真实超过档位阈值的单次请求也无法按高档计价——源头没有单次上下文，无法区分。刷新统计后费用按基础价重算。
