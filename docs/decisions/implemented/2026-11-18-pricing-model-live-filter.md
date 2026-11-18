# DR: 定价页模型筛选即时过滤

Status: implemented

## Problem

定价页模型筛选使用 `CompactSelect`，但面板搜索框只在面板内部过滤候选列表，从不向外更新筛选值；只有点击某个候选模型，`searchModel` 才更新并触发下方卡片过滤。用户希望打字输入模型名或别名时，下方模型动态过滤，不必先点击候选项。

## Decision

`CompactSelect` 新增可选 prop `liveFilter`（默认 `false`）。启用后面板搜索输入即时作为值向外更新（`emit('update:modelValue', value)`），候选列表由父级按同一搜索词过滤，组件不再二次过滤，保证面板候选与外部结果同口径。定价页模型筛选启用该模式；`modelOptions` 复用 `matchesModelSearch`（模型名、历史／用户别名、统一解析器正则，ASCII 大小写不敏感），与下方卡片共用同一匹配逻辑；新增无匹配空态。家族筛选本次保持点击选择。匹配逻辑抽到 `src/utils/modelSearch.ts` 并补单元测试。

## Alternatives considered

**修改 `CompactSelect` 默认行为为输入即筛选：** 会波及 `FilterBar`、`RealtimeToken`、`TaskCreateDialog`、`CursorCsvPreviewDialog` 等调用处，把「选择式」筛选变成「输入式」，可能让全局 `modelId` / 数据源查询被输入文本污染。

**在定价页另用 Naive UI AutoComplete / Select：** 视觉与工具栏既有 `CompactSelect` 不一致，需要重复实现候选面板、定位与键盘交互。

**仅把面板搜索词 emit 给父级，`options` 仍只按家族过滤：** 面板过滤（label/value 直接包含）与卡片匹配（别名、正则解析）口径不一致，输入别名时面板显示「无匹配项」而下方实际有结果，造成误导。

**家族筛选一并改为即时过滤：** 家族选项少，当前按 family id 精确匹配；改输入过滤需引入 label/id 模糊匹配并调整 `familyGroups` 的家族筛选语义，超出本次需求范围。

## Consequences

- 换来：模型框打字即实时过滤下方卡片；点击候选仍精确定位单个模型；清空恢复全部；无匹配时显示空态。
- 代价：`CompactSelect` 多一个可选 prop 与分支契约——启用 `liveFilter` 的调用处必须自行按搜索词过滤 `options`，否则面板会显示未过滤列表；家族筛选与模型筛选交互暂不一致。
- 其他 `CompactSelect` 调用处不传该 prop，行为完全不变。
