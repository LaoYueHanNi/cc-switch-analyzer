# CC-Switch 使用分析器 — 完整开发计划

> **历史文档**: 本文档基于 Electron 架构编写，项目已迁移到 Tauri v2。
> 当前架构请参考 [README.md](../README.md)。本文档保留作为设计参考。

> 基于 `docs/PRD.md` 产品需求文档制定
> 参考实现：`/Users/laoyuehanni/Desktop/cc-switch-analyzer`（Java + JavaFX 版本）

---

## 一、技术选型

| 类别 | 选型 | 理由 |
|------|------|------|
| **构建工具** | electron-vite | Electron 官方 Vite 脚手架，开箱即用的 main/preload/renderer 三进程架构 |
| **UI 框架** | **Naive UI** | 90+ 组件、全量 TypeScript、tree-shakable、无 CSS 依赖、主题可定制、日期选择器/下拉/弹窗等组件齐全 |
| **图表库** | **ECharts** (via vue-echarts) | 面积图、热力图功能完善；平滑曲线(Catmull-Rom)原生支持；中文文档优秀 |
| **SQLite** | **better-sqlite3** | 同步 API 性能优秀；适合 Electron 主进程；需 `@electron/rebuild` 适配 |
| **状态管理** | **Pinia** | Vue 3 官方推荐，TypeScript 原生支持，模块化 store |
| **图标** | **@vicons/ionicons5** | Naive UI 官方推荐，轻量 |
| **包管理器** | **pnpm** | 节省磁盘空间，依赖管理严格 |

---

## 二、目录结构

```
cc-switch-analyzer-electron/
├── electron.vite.config.ts
├── package.json
├── tsconfig.json
├── tsconfig.node.json
├── tsconfig.web.json
├── resources/                        # 应用图标等静态资源
├── src/
│   ├── main/                         # Electron 主进程
│   │   ├── index.ts                  # 入口：app 生命周期、窗口创建
│   │   ├── window.ts                 # 窗口管理（尺寸、状态持久化）
│   │   ├── ipc/
│   │   │   ├── index.ts              # IPC 注册入口
│   │   │   ├── database.ipc.ts       # 数据库操作 IPC
│   │   │   └── dialog.ipc.ts         # 文件对话框 IPC
│   │   ├── services/
│   │   │   ├── external-db.ts        # 外部 CC-Switch 数据库（只读）
│   │   │   ├── app-db.ts             # 应用自有数据库（读写）
│   │   │   ├── pricing-engine.ts     # 定价计算引擎
│   │   │   └── precompute.ts         # 预计算服务
│   │   └── utils/
│   │       ├── format.ts             # 数值格式化
│   │       └── constants.ts          # 常量定义（颜色、默认值等）
│   ├── preload/
│   │   ├── index.ts                  # preload 脚本
│   │   └── index.d.ts               # 暴露给 renderer 的 API 类型声明
│   └── renderer/                     # Vue 3 渲染进程
│       ├── index.html
│       └── src/
│           ├── App.vue
│           ├── main.ts
│           ├── router/
│           │   └── index.ts
│           ├── stores/
│           │   ├── database.ts
│           │   ├── filter.ts
│           │   ├── pricing.ts
│           │   └── realtime.ts
│           ├── composables/
│           │   ├── useDatabase.ts
│           │   ├── usePricing.ts
│           │   ├── useFilter.ts
│           │   ├── useAutoRefresh.ts
│           │   └── useRealtimePolling.ts
│           ├── components/
│           │   ├── layout/
│           │   │   ├── AppLayout.vue
│           │   │   ├── Toolbar.vue
│           │   │   ├── FilterBar.vue
│           │   │   └── SummaryBar.vue
│           │   ├── common/
│           │   │   ├── StatCard.vue
│           │   │   ├── PricingGrid.vue
│           │   │   └── CacheWindowDialog.vue
│           │   └── charts/
│           │       ├── RealtimeAreaChart.vue
│           │       └── DensityChart.vue
│           ├── views/
│           │   ├── ByModel.vue
│           │   ├── ByProvider.vue
│           │   ├── SessionAnalysis.vue
│           │   ├── RealtimeToken.vue
│           │   └── PricingCalculator.vue
│           ├── components/
│           │   ├── model/
│           │   │   ├── ModelCard.vue
│           │   │   └── ModelCompareDialog.vue
│           │   ├── provider/
│           │   │   └── ProviderCard.vue
│           │   ├── session/
│           │   │   ├── SessionCard.vue
│           │   │   └── ModelBreakdown.vue
│           │   └── pricing/
│           │       ├── PricingCard.vue
│           │       ├── PricingEditForm.vue
│           │       └── TimePricingDialog.vue
│           ├── utils/
│           │   ├── format.ts
│           │   └── constants.ts
│           └── types/
│               ├── database.ts
│               ├── pricing.ts
│               └── common.ts
└── docs/
    ├── PRD.md
    └── DEVELOPMENT_PLAN.md
```

---

## 三、数据库结构（严格参考 Java 实现）

### 3.1 外部数据库 cc-switch.db（只读）

应用**严禁修改**此数据库的任何内容（包括 DDL 操作）。

**表 `proxy_request_logs`：**

| 列名 | 类型 | 说明 |
|------|------|------|
| provider_id | TEXT | 供应商标识 |
| app_type | TEXT | 应用类型 |
| model | TEXT | 模型名称 |
| created_at | INTEGER | Unix 秒时间戳 |
| status_code | INTEGER | HTTP 状态码 |
| input_tokens | INTEGER | 输入 Token |
| output_tokens | INTEGER | 输出 Token |
| cache_read_tokens | INTEGER | 缓存读取 Token |
| cache_creation_tokens | INTEGER | 缓存写入 Token |
| total_cost_usd | TEXT | 总费用（USD，TEXT 存储） |
| input_cost_usd | TEXT | 输入费用 |
| output_cost_usd | TEXT | 输出费用 |
| cache_read_cost_usd | TEXT | 缓存读取费用 |
| cache_creation_cost_usd | TEXT | 缓存写入费用 |
| latency_ms | INTEGER | 延迟毫秒 |
| session_id | TEXT | 会话 ID |

**表 `providers`：**

| 列名 | 类型 |
|------|------|
| id | TEXT |
| app_type | TEXT |
| name | TEXT |

**表 `model_pricing`：**

| 列名 | 类型 | 说明 |
|------|------|------|
| model_id | TEXT | 模型标识 |
| display_name | TEXT | 显示名称 |
| input_cost_per_million | REAL | 每百万输入 Token 费用（USD） |
| output_cost_per_million | REAL | 每百万输出 Token 费用（USD） |
| cache_read_cost_per_million | REAL | 每百万缓存读取费用（USD） |
| cache_creation_cost_per_million | REAL | 每百万缓存写入费用（USD） |

### 3.2 应用自有数据库（读写）

存储路径：`~/.cc-switch-analyzer/pricing.db`（自动创建目录和表）

```sql
CREATE TABLE IF NOT EXISTS pricing_overrides (
    model_id TEXT PRIMARY KEY,
    input_cost_per_million REAL NOT NULL,
    output_cost_per_million REAL NOT NULL,
    cache_read_cost_per_million REAL NOT NULL,
    cache_creation_cost_per_million REAL NOT NULL,
    updated_at INTEGER DEFAULT (strftime('%s','now'))
);

CREATE TABLE IF NOT EXISTS time_pricing_overrides (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_id TEXT NOT NULL,
    start_time INTEGER NOT NULL,
    end_time INTEGER NOT NULL,
    input_cost_per_million REAL NOT NULL,
    output_cost_per_million REAL NOT NULL,
    cache_read_cost_per_million REAL NOT NULL,
    cache_creation_cost_per_million REAL NOT NULL,
    label TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER DEFAULT (strftime('%s','now'))
);
```

---

## 四、核心 SQL 查询（参考 Java 版 DatabaseService）

### 4.1 筛选参数构建

```typescript
// FilterParams 动态构建 WHERE 子句
function buildWhereClause(params: FilterParams, aliased: boolean): { sql: string, binds: any[] } {
  const prefix = aliased ? 'l.' : '';
  const clauses: string[] = ['1=1'];
  const binds: any[] = [];

  if (params.fromDate) {
    clauses.push(`${prefix}created_at >= ?`);
    binds.push(toEpochSeconds(params.fromDate));
  }
  if (params.toDate) {
    clauses.push(`${prefix}created_at < ?`);  // 注意：to 是 exclusive，需要 +1 天
    binds.push(toEpochSeconds(params.toDate.plusDays(1)));
  }
  if (params.providerId) {
    clauses.push(`${prefix}provider_id = ?`);
    binds.push(params.providerId);
  }
  if (params.modelId) {
    clauses.push(`${prefix}model = ?`);
    binds.push(params.modelId);
  }

  return { sql: `WHERE ${clauses.join(' AND ')}`, binds };
}
```

### 4.2 供应商查询（JOIN providers 表）

```sql
SELECT DISTINCT l.provider_id, COALESCE(p.name, l.provider_id) AS name
FROM proxy_request_logs l
LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
ORDER BY name
```

### 4.3 模型列表

```sql
SELECT DISTINCT model FROM proxy_request_logs ORDER BY model
```

### 4.4 日期范围

```sql
SELECT MIN(created_at), MAX(created_at) FROM proxy_request_logs
```

### 4.5 摘要统计

```sql
SELECT
    COUNT(*) AS total_requests,
    SUM(CASE WHEN status_code=200 THEN 1 ELSE 0 END) AS success_count,
    SUM(input_tokens) AS total_input,
    SUM(output_tokens) AS total_output,
    SUM(cache_read_tokens) AS total_cache_read,
    SUM(cache_creation_tokens) AS total_cache_creation,
    ROUND(AVG(latency_ms), 0) AS avg_latency
FROM proxy_request_logs
WHERE {dynamic_filters}
```

### 4.6 模型维度统计

```sql
SELECT
    l.model,
    COUNT(*) AS requests,
    SUM(l.input_tokens) AS input_tokens,
    SUM(l.output_tokens) AS output_tokens,
    SUM(l.cache_read_tokens) AS cache_read,
    SUM(l.cache_creation_tokens) AS cache_creation
FROM proxy_request_logs l
WHERE {dynamic_filters_with_l_prefix}
GROUP BY l.model
ORDER BY requests DESC
```

### 4.7 供应商维度统计

```sql
SELECT
    COALESCE(p.name, l.provider_id) AS provider_name,
    l.provider_id,
    COUNT(*) AS requests,
    SUM(CASE WHEN l.status_code=200 THEN 1 ELSE 0 END) AS successes,
    ROUND(100.0 * SUM(CASE WHEN l.status_code=200 THEN 1 ELSE 0 END) / COUNT(*), 1) AS success_rate,
    ROUND(AVG(l.latency_ms), 0) AS avg_latency
FROM proxy_request_logs l
LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
WHERE {dynamic_filters_with_l_prefix}
GROUP BY l.provider_id
ORDER BY requests DESC
```

### 4.8 供应商-模型 Token 映射（用于供应商费用计算）

```sql
SELECT
    l.provider_id, l.model,
    SUM(l.input_tokens) AS input_tokens,
    SUM(l.output_tokens) AS output_tokens,
    SUM(l.cache_read_tokens) AS cache_read,
    SUM(l.cache_creation_tokens) AS cache_creation
FROM proxy_request_logs l
WHERE {dynamic_filters_with_l_prefix}
GROUP BY l.provider_id, l.model
```

### 4.9 每日趋势（用于预计算）

```sql
SELECT
    date(l.created_at, 'unixepoch') AS day,
    l.model,
    COUNT(*) AS requests,
    SUM(l.input_tokens) AS input_tokens,
    SUM(l.output_tokens) AS output_tokens,
    SUM(l.cache_read_tokens) AS cache_read,
    SUM(l.cache_creation_tokens) AS cache_creation,
    ROUND(AVG(l.latency_ms), 0) AS avg_latency
FROM proxy_request_logs l
WHERE {dynamic_filters_with_l_prefix}
GROUP BY day, l.model
ORDER BY day
```

### 4.10 缓存窗口平均时长（SQL 窗口函数）

```sql
WITH marked AS (
    SELECT l.session_id, l.model, l.created_at, l.cache_read_tokens,
        CASE WHEN l.cache_read_tokens = 0 THEN 1 ELSE 0 END AS window_break
    FROM proxy_request_logs l
    WHERE {dynamic_filters}
    AND l.created_at >= ?  -- 最近30天
),
grouped AS (
    SELECT session_id, model, created_at, cache_read_tokens,
        SUM(window_break) OVER (
            PARTITION BY session_id, model ORDER BY created_at
        ) AS grp
    FROM marked
),
window_durations AS (
    SELECT model, grp,
        MAX(created_at) - MIN(created_at) AS duration_sec,
        MAX(created_at) AS end_ts
    FROM grouped
    WHERE cache_read_tokens > 0
    GROUP BY session_id, model, grp
    HAVING COUNT(*) > 1
),
ranked AS (
    SELECT model, duration_sec,
        ROW_NUMBER() OVER (PARTITION BY model ORDER BY end_ts DESC) AS rn
    FROM window_durations
)
SELECT model, AVG(duration_sec) AS avg_duration_sec
FROM ranked WHERE rn <= 10 GROUP BY model
```

### 4.11 单模型缓存窗口详情

与 4.10 类似，但固定 `model = ?`，返回最近 10 个窗口的开始/结束时间、持续时长、命中次数。

### 4.12 会话统计（Top 50）

```sql
-- 先查 Top 50 session_id（按请求数排序）
SELECT session_id FROM proxy_request_logs
WHERE {dynamic_filters} AND session_id IS NOT NULL AND session_id != ''
GROUP BY session_id
ORDER BY COUNT(*) DESC
LIMIT 50
```

然后分别查询：
- 会话级聚合（请求数、Token 总量、最大上下文宽度等）
- 会话-模型维度 Token 分解
- 会话内每条请求的时间戳和 Token（用于时间感知定价计算）

### 4.13 实时 Token 趋势（10 秒桶）

```sql
SELECT (created_at / 10) * 10 AS bucket,
       COUNT(*) AS requests,
       SUM(input_tokens) AS input_tokens,
       SUM(output_tokens) AS output_tokens,
       SUM(cache_read_tokens) AS cache_read,
       SUM(cache_creation_tokens) AS cache_creation
FROM proxy_request_logs
WHERE created_at >= ?
GROUP BY bucket
ORDER BY bucket
```

---

## 五、定价引擎设计（参考 Java 版 PricingService）

### 5.1 三层定价优先级

```
时间定价规则（最高优先级，RMB 直存）
  ↓ 无匹配时回退
用户固定覆盖（RMB 直存）
  ↓ 无覆盖时回退
基础定价 × 汇率（USD × exchangeRate → RMB）
```

### 5.2 定价合并逻辑

```typescript
function merge(
  base: Map<string, ModelPricing>,   // 基础定价 × 汇率（已转为 RMB）
  overrides: Map<string, PricingOverride>  // 用户覆盖（已是 RMB）
): Map<string, ModelPricing> {
  const result = new Map(base);
  for (const [modelId, ov] of overrides) {
    const baseEntry = base.get(modelId);
    const displayName = baseEntry?.displayName ?? modelId;
    result.set(modelId, {
      modelId,
      displayName,
      inputCostPerMillion: ov.inputCostPerMillion,
      outputCostPerMillion: ov.outputCostPerMillion,
      cacheReadCostPerMillion: ov.cacheReadCostPerMillion,
      cacheCreationCostPerMillion: ov.cacheCreationCostPerMillion,
      isOverride: true
    });
  }
  return result;
}
```

### 5.3 时间感知定价查询

```typescript
function getPricingAt(modelId: string, epochSeconds: number): ModelPricing | null {
  // 1. 检查时间定价规则
  const timeRules = timeOverridesByModel.get(modelId);
  if (timeRules) {
    for (const rule of timeRules) {
      if (rule.startTime <= epochSeconds && epochSeconds <= rule.endTime) {
        return {
          modelId,
          displayName: merged.get(modelId)?.displayName ?? modelId,
          inputCostPerMillion: rule.inputCostPerMillion,
          outputCostPerMillion: rule.outputCostPerMillion,
          cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
          cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
          isOverride: false
        };
      }
    }
  }
  // 2. 回退到合并后的固定定价
  return merged.get(modelId) ?? null;
}
```

### 5.4 费用计算公式

```typescript
function calculateCost(pricing: ModelPricing, tokens: TokenDimensions): number {
  return (tokens.input * pricing.inputCostPerMillion
        + tokens.output * pricing.outputCostPerMillion
        + tokens.cacheRead * pricing.cacheReadCostPerMillion
        + tokens.cacheCreation * pricing.cacheCreationCostPerMillion
        ) / 1_000_000;
}
```

### 5.5 预计算机制（参考 Java 版 precomputeCosts）

**一次遍历** `dailyTrend` 数据，产出：

| 预计算结果 | 用途 |
|-----------|------|
| `modelCosts: Map<string, number>` | 每个模型的总费用 |
| `modelCostBreakdown: Map<string, [number, number, number, number]>` | 每个模型的 [input, output, cacheRead, cacheCreation] 费用 |
| `providerCosts: Map<string, number>` | 每个供应商的费用（通过 providerModelTokens 映射） |
| `dayCostMap: Map<string, number>` | 每天的总费用 |
| `dayRequestsMap: Map<string, number>` | 每天的请求数 |
| `dailyByModel: Map<string, DailyData[]>` | 每个模型的每日数据列表 |

遍历逻辑：
```typescript
for (const row of dailyTrend) {
  const epoch = parseDayToEpochSeconds(row.day);
  const pricing = pricingEngine.getPricingAt(row.model, epoch);  // 时间感知定价
  if (pricing) {
    const dayCosts = [
      row.inputTokens * pricing.inputCostPerMillion / 1_000_000,
      row.outputTokens * pricing.outputCostPerMillion / 1_000_000,
      row.cacheRead * pricing.cacheReadCostPerMillion / 1_000_000,
      row.cacheCreation * pricing.cacheCreationCostPerMillion / 1_000_000,
    ];
    // 累加到 modelCosts、modelCostBreakdown、dayCostMap 等
  }
}
// 然后遍历 providerModelTokens，将模型费用按比例分配到供应商
```

---

## 六、IPC 通道设计

### 6.1 数据库操作

| 通道名 | 参数 | 返回值 | 说明 |
|--------|------|--------|------|
| `db:select-file` | — | `{path, recordCount}` | 弹出文件选择器（过滤 .db）并加载 |
| `db:load` | `filePath` | `{recordCount, dateRange, providers, models}` | 加载指定数据库 |
| `db:refresh` | — | `{hasNew, recordCount}` | 检测是否有新数据（比较 max created_at） |
| `db:get-filter-options` | — | `{providers, models, dateRange}` | 获取筛选下拉选项 |

### 6.2 数据查询

| 通道名 | 参数 | 返回值 | 说明 |
|--------|------|--------|------|
| `query:summary` | `FilterParams` | `SummaryData` | 摘要统计 |
| `query:by-model` | `FilterParams` | `ModelBreakdown[]` | 模型维度统计 |
| `query:by-provider` | `FilterParams` | `ProviderBreakdown[]` | 供应商维度统计 |
| `query:provider-model-tokens` | `FilterParams` | `ProviderModelToken[]` | 供应商-模型 Token 映射 |
| `query:daily-trend` | `FilterParams` | `DailyTrendRow[]` | 每日趋势（用于预计算） |
| `query:cache-durations` | `FilterParams` | `Map<string, number>` | 各模型平均缓存时长 |
| `query:cache-windows` | `modelId` | `CacheWindow[]` | 单模型缓存窗口详情 |
| `query:sessions` | `{filter, sortBy}` | `SessionStat[]` | Top 20 会话 |
| `query:session-model-tokens` | `{filter, sessionIds}` | `SessionModelToken[]` | 会话-模型 Token 分解 |
| `query:session-request-tokens` | `{filter, sessionIds}` | `SessionRequestToken[]` | 会话请求级 Token（用于时间感知定价） |
| `query:session-timestamps` | `sessionIds` | `Map<string, number[]>` | 会话请求时间戳（用于密度图） |
| `query:realtime` | — | `RealtimeBucket[]` | 最近 1 小时 10 秒桶数据 |

### 6.3 定价操作

| 通道名 | 参数 | 返回值 |
|--------|------|--------|
| `pricing:get-all` | — | `PricingData[]` |
| `pricing:set-override` | `{modelId, input, output, cacheRead, cacheCreation}` | `void` |
| `pricing:remove-override` | `modelId` | `void` |
| `pricing:add-time-rule` | `{modelId, startTime, endTime, input, output, cacheRead, cacheCreation, label}` | `ruleId` |
| `pricing:update-time-rule` | `{id, startTime, endTime, input, output, cacheRead, cacheCreation, label}` | `void` |
| `pricing:delete-time-rule` | `id` | `void` |
| `pricing:set-exchange-rate` | `number` | `void` |
| `pricing:get-exchange-rate` | — | `number` |
| `pricing:refresh` | — | `void` |

### 6.4 对话框

| 通道名 | 参数 | 返回值 |
|--------|------|--------|
| `dialog:open-file` | `{filters}` | `filePath \| null` |

---

## 七、核心类型定义

```typescript
// === types/database.ts ===

export interface FilterParams {
  fromDate: Date | null       // 起始日期（含）
  toDate: Date | null         // 结束日期（含，SQL 中转为 exclusive +1 天）
  providerId: string          // 空字符串 = 全部
  modelId: string             // 空字符串 = 全部
}

export interface Provider {
  id: string
  appType: string
  name: string
}

export interface ModelPricing {
  modelId: string
  displayName: string
  inputCostPerMillion: number       // USD（基础）或 RMB（覆盖）
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
}

export interface SummaryData {
  totalRequests: number
  successCount: number
  totalInput: number
  totalOutput: number
  totalCacheRead: number
  totalCacheCreation: number
  avgLatency: number
}

export interface ModelBreakdown {
  model: string
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface ProviderBreakdown {
  providerName: string
  providerId: string
  requests: number
  successes: number
  successRate: number
  avgLatency: number
}

export interface ProviderModelToken {
  providerId: string
  model: string
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface DailyTrendRow {
  day: string          // YYYY-MM-DD
  model: string
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
  avgLatency: number
}

export interface RealtimeBucket {
  bucket: number       // Unix 秒（10 秒对齐）
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

// === types/pricing.ts ===

export interface MergedPricing extends ModelPricing {
  isOverride: boolean
}

export interface PricingOverride {
  modelId: string
  inputCostPerMillion: number     // RMB
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  updatedAt: number
}

export interface TimePricingRule {
  id: number
  modelId: string
  startTime: number               // Unix 秒
  endTime: number                 // Unix 秒
  inputCostPerMillion: number     // RMB
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  label: string
}

export interface PricingData {
  modelId: string
  displayName: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  isOverride: boolean
  hasTimePricing: boolean
  timeRules: TimePricingRule[]
  isUsed: boolean                 // 是否有实际请求
}

// === types/common.ts ===

export interface PrecomputedResult {
  modelCosts: Map<string, number>
  modelCostBreakdown: Map<string, [number, number, number, number]>
  providerCosts: Map<string, number>
  dayCostMap: Map<string, number>
  dayRequestsMap: Map<string, number>
  dailyByModel: Map<string, DailyTrendRow[]>
}

export interface CacheWindow {
  startTime: number
  endTime: number
  durationSec: number
  hitCount: number
}

export interface SessionStat {
  sessionId: string
  requestCount: number
  totalTokens: number
  maxContextWidth: number
  startTime: number
  endTime: number
  cacheHitRate: number
  modelBreakdown: SessionModelToken[]
  timestamps: number[]
  // 计算字段
  totalCost: number
  durationSec: number
}

export interface SessionModelToken {
  sessionId: string
  model: string
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface SessionRequestToken {
  sessionId: string
  model: string
  createdAt: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}
```

---

## 八、UI 常量（参考 Java 版 Styles.java）

### 8.1 功能色

```typescript
export const COLORS = {
  COST_RED: '#e74c3c',      // 费用文字
  PRIMARY_BLUE: '#4a90d9',  // 主蓝色
  GREEN: '#27ae60',          // 成功
  PURPLE: '#8e44ad',         // 输入 Token
  ORANGE: '#f39c12',         // 输出 Token
  TEAL: '#16a085',           // Token 总量
  BLUE: '#2980b9',           // 缓存读取
  DARK_ORANGE: '#d35400',    // 缓存写入
} as const;
```

### 8.2 摘要统计条 8 项指标

| 标签 | 颜色 | 数据键 |
|------|------|--------|
| 总请求数 | PRIMARY_BLUE | totalRequests |
| 成功请求数 | GREEN | successCount |
| 总费用（¥） | COST_RED | totalCost |
| 输入 Token | PURPLE | totalInput |
| 输出 Token | ORANGE | totalOutput |
| 平均延迟(ms) | TEAL | avgLatency |
| 缓存命中 Token | BLUE | totalCacheRead |
| 缓存写入 Token | DARK_ORANGE | totalCacheCreation |

---

## 九、格式化工具函数（参考 Java 版 UiUtils.java）

```typescript
// 大数 K/M 简写
export function formatNum(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
  return String(n);
}

// 费用格式化：¥X.XX，2-4 位小数，去尾零
export function formatCost(cny: number): string {
  return '¥' + formatRate(cny);
}

// 定价单价格式化：2-4 位小数，去尾零，保底 2 位
export function formatRate(v: number): string {
  let s = v.toFixed(4);
  const dot = s.indexOf('.');
  if (dot < 0) return s;
  s = s.replace(/0+$/, '');
  if (s.endsWith('.')) s += '00';
  const minEnd = dot + 3;  // 至少 2 位小数
  while (s.length < minEnd) s += '0';
  return s;
}

// 时长格式化
export function formatDuration(seconds: number): string {
  if (seconds < 60) return seconds + 's';
  if (seconds < 3600) return Math.floor(seconds / 60) + 'm';
  return Math.floor(seconds / 3600) + 'h ' + Math.floor((seconds % 3600) / 60) + 'm';
}
```

---

## 十、开发阶段

### 阶段 0：项目初始化

**目标**：搭建可运行的 Electron + Vue 3 + TypeScript 空项目

**任务**：
1. 使用 `npm create @quick-start/electron` 初始化（选择 Vue + TypeScript）
2. 安装核心依赖：
   ```
   pnpm add naive-ui @vicons/ionicons5 vue-echarts echarts pinia vue-router
   pnpm add better-sqlite3
   pnpm add -D @electron/rebuild @types/better-sqlite3
   ```
3. 运行 `npx @electron/rebuild -f -w better-sqlite3`
4. 配置 electron-vite（别名 `@`、路径映射）
5. 建立目录结构（创建所有目录和空文件占位）
6. 验证 `pnpm dev` 启动空白窗口

**验证**：`pnpm dev` 启动空白 Electron 窗口，渲染 Vue 默认页面

---

### 阶段 1：窗口管理 + 主布局骨架

**目标**：窗口状态持久化、Tab 路由、空状态提示、顶部区域动态切换

**参考**：Java 版 `CcSwitchAnalyzer.java`（窗口状态）、`MainController.createView()`（Tab + 顶部区域切换）

**任务**：

1. **窗口管理** (`src/main/window.ts`)
   - 默认 1280×860，标题 "CC-Switch 使用分析器"
   - 窗口状态持久化（位置/大小/最大化），使用 `electron-window-state` 或 `electron-store`
   - 键名参考：`win.x`, `win.y`, `win.w`, `win.h`, `win.maximized`

2. **主布局** (`AppLayout.vue`)
   - 顶部工具栏区域（动态切换：默认工具栏+筛选+摘要 / 定价Tab工具栏 / 会话排序栏 / 实时统计卡片）
   - Tab 栏：5 个 Tab
   - 内容区（router-view）
   - 未加载数据库时禁用所有控件

3. **Tab 路由** (`router/index.ts`)
   - 5 个路由（hash 模式）：`/by-model`, `/by-provider`, `/session`, `/realtime`, `/pricing`
   - Tab 切换时触发顶部区域替换

4. **空状态与加载**
   - 加载中：ProgressIndicator 旋转动画 + "正在加载数据库..."
   - 无数据库："请选择数据库文件" 提示
   - 使用 Stack 叠加层实现

**验证**：启动后显示 Tab 栏和空状态提示，Tab 切换正常，窗口关闭重开恢复位置尺寸

---

### 阶段 2：数据层 — 双数据库 + IPC

**目标**：完成 external-db、app-db、所有 IPC 通道注册

**参考**：Java 版 `DatabaseService.java`（548 行）、`AppDatabaseService.java`（206 行）

**任务**：

1. **类型定义** (`types/`) — 见第七节完整类型

2. **ExternalDbService** (`services/external-db.ts`)
   - better-sqlite3 连接管理
   - 实现第四节的全部 13 个 SQL 查询
   - 所有查询支持动态 FilterParams
   - **严格只读**，不执行任何写操作

3. **AppDbService** (`services/app-db.ts`)
   - 自动创建 `~/.cc-switch-analyzer/pricing.db` 和 3 张表
   - CRUD：settings（汇率等）
   - CRUD：pricing_overrides
   - CRUD：time_pricing_overrides
   - INSERT OR REPLACE 模式

4. **IPC 注册** (`ipc/`)
   - `database.ipc.ts`：注册所有数据库和查询 IPC handler
   - `dialog.ipc.ts`：注册文件选择对话框
   - 版本号机制防竞态（参考 Java 版 `queryVersion`）

5. **Preload** (`preload/index.ts` + `index.d.ts`)
   - contextBridge 暴露所有 IPC 为类型安全 API

**验证**：从 renderer 调用 preload API 能打开 .db 文件、读取记录数、执行筛选查询

---

### 阶段 3：定价引擎 + 预计算

**目标**：三层定价、时间感知定价、预计算机制

**参考**：Java 版 `PricingService.java`（203 行）、`MainController.precomputeCosts()`

**任务**：

1. **PricingEngine** (`services/pricing-engine.ts`) — 见第五节完整设计
   - `refresh()`：重新加载汇率、基础定价、覆盖、时间规则，重新合并
   - `getPricing(modelId)`：从合并结果获取固定定价
   - `getPricingAt(modelId, epochSeconds)`：时间感知定价
   - `calculateCost(pricing, tokens)`：费用计算

2. **PrecomputeService** (`services/precompute.ts`) — 见第五节预计算机制
   - 接收 dailyTrend + providerModelTokens + pricingEngine
   - 一次遍历产出 PrecomputedResult
   - 会话费用使用请求级时间感知定价（每条请求单独查定价）

3. **定价 IPC**
   - 注册所有定价操作 IPC
   - 级联刷新链路：修改 → 保存 DB → `refresh()` → `precompute()` → 通知 renderer

**验证**：给定测试数据，三层优先级计算正确；预计算结果与手动计算一致

---

### 阶段 4：全局 UI — 工具栏 + 筛选 + 摘要

**目标**：全局工具栏、筛选系统、摘要统计条、数值格式化

**参考**：Java 版 `FilterBar.java`（123 行）、`SummaryBar.java`（56 行）、`Toolbar`（MainController 内）

**任务**：

1. **Pinia Stores**
   - `database.ts`：dbPath、recordCount、isLoading、isLoaded
   - `filter.ts`：fromDate、toDate、providerId、modelId、providerOptions、modelOptions、dateRange
   - `pricing.ts`：pricingData、exchangeRate、timeRules

2. **Toolbar.vue**
   - 左侧：选择数据库按钮 + dbPath 标签（`文件路径... | 共 N 条记录`）
   - 右侧：刷新按钮 + 自动刷新下拉（手动/30s/1min/5min/30min）
   - 未加载时所有按钮 disabled

3. **FilterBar.vue**
   - Naive UI NDatePicker（日期范围）+ NSelect（供应商、模型）
   - 快捷日期按钮：近 1/7/30/60/180 天（点击立即触发查询）
   - 查询/重置按钮
   - 首次加载日期范围设为数据的最小~最大日期

4. **SummaryBar.vue**
   - 8 个 StatCard，参考第八节颜色映射
   - 总费用从预计算结果获取

5. **数值格式化** (`utils/format.ts`) — 见第九节

6. **通用组件**
   - `StatCard.vue`：标题 + 数值 + 底部颜色条
   - `PricingGrid.vue`：四维费用分解（4 行，每行：颜色圆点 + 标签 + Token 数 + 费用 + 单价）

**验证**：加载数据库后筛选正常、摘要正确、快捷日期触发查询

---

### 阶段 5：Tab 1 — 按模型统计

**目标**：模型卡片流、费用分解、模型对比、缓存窗口

**参考**：Java 版 `ModelTabView.java`（574 行）

**任务**：

1. **ModelCard.vue**
   - 模型名称（粗体）+ 总费用（红色大号可点击）+ 总 Token（青色大号）
   - 单次请求费用 = 总费用 / 请求数
   - 缓存命中率 = cacheRead / (input + cacheRead + cacheWrite)
   - 平均缓存时长（可点击，弹出窗口）
   - 费用分解网格（复用 PricingGrid）
   - 无定价时显示"暂无定价数据"+"设置定价"按钮（跳转定价 Tab）
   - 有时间定价时显示时钟图标 + "包含时段定价"

2. **ModelCompareDialog.vue**
   - 可搜索下拉列出所有其他模型
   - 对比视图：当前 Token × 目标模型定价 = 转换费用
   - 差异百分比（+15.3% 或 -8.7%）
   - 四维对比行（蓝色高亮）
   - 恢复按钮

3. **CacheWindowDialog.vue**
   - 最近 10 个缓存命中窗口
   - 列：开始时间、结束时间、持续时长、命中次数

4. **ByModel.vue**
   - 卡片流布局（CSS Grid auto-fill）
   - 按请求数降序

**验证**：多模型数据正确展示，点击费用触发对比，点击缓存时长弹出窗口

---

### 阶段 6：Tab 2 — 按供应商统计

**目标**：供应商卡片，费用通过模型-供应商映射计算

**参考**：Java 版 `ProviderTabView.java`（110 行）

**任务**：

1. **ProviderCard.vue**
   - 供应商名称（LEFT JOIN providers 表获取显示名，无则用 provider_id）
   - 总费用（红色大号）
   - 请求数

2. **ByProvider.vue**
   - 卡片流布局，按请求数降序
   - 费用 = 预计算中的 providerCosts（通过 providerModelTokens 映射）

**验证**：供应商费用 = 其下各模型费用之和

---

### 阶段 7：Tab 3 — 会话分析

**目标**：Top 20 会话卡片、密度热力图、模型分解、时间感知定价

**参考**：Java 版 `SessionTabView.java`（543 行）

**任务**：

1. **排序下拉**（替换顶部工具栏区域）
   - 费用（默认）/ Token / 请求数 / 上下文大小 / 缓存命中率
   - 全部降序

2. **SessionCard.vue** — 三区域水平布局
   - **概览区**（左侧固定 ~160px）：
     - 短 ID（首段，悬停全名）
     - 总费用（红色）+ Token 数（青色）
     - "N 次请求, 持续 Xm"
     - 时间范围 "MM/DD HH:mm ~ HH:mm"
     - 上下文大小 = max(输入+缓存读取)，K/M 格式化
     - 缓存命中率
   - **密度热力图**（中间固定 ~160px）：ECharts 面积图，32 个时间桶，红色渐变填充
   - **模型分解区**（右侧弹性宽度）：水平排列各模型块

3. **DensityChart.vue**
   - ECharts 面积图
   - 将时间戳分 32 桶，绘制密度曲线
   - 红色渐变填充，两端时间标签
   - 悬停提示：总请求数、峰值时间、峰值请求数

4. **ModelBreakdown.vue**
   - 每个模型块（min-width: 140px）：
     - 模型名（截断 20 字符）
     - 输入/输出/缓存读取/缓存写入：Token 数 + 费用
     - 缓存命中率
     - 模型总费用（红色）

5. **SessionAnalysis.vue**
   - Top 50 中取 Top 20
   - 费用计算：每条请求用 `getPricingAt(model, createdAt)` 时间感知定价

**验证**：会话卡片三区域渲染正确，密度图有数据，排序切换正确

---

### 阶段 8：Tab 4 — 实时 Token 监控

**目标**：实时面积图、10s 轮询、LIVE 标识

**参考**：Java 版 `RealtimeTabView.java`（191 行）

**任务**：

1. **顶部统计卡片**（替换工具栏区域）
   - 近 1 小时 Token 数、请求数、上次刷新时间

2. **RealtimeAreaChart.vue**
   - ECharts 面积图
   - X 轴：时间（分钟精度），锁定恰好 1 小时窗口（对齐到分钟边界）
   - Y 轴：Token 数（自动缩放，K/M 格式化）
   - 数据粒度：10 秒桶
   - 平滑曲线：ECharts `smooth: true` 或自定义 Catmull-Rom（参考 Java 版 `catmullRom()` 方法）
   - 蓝色半透明填充 + 蓝色描边
   - LIVE 标识：红色文字 + CSS 脉冲动画圆点

3. **轮询机制** (`composables/useRealtimePolling.ts`)
   - 进入 Tab → `setInterval(10s)` → 调用 `query:realtime`
   - 离开 Tab → `clearInterval`
   - 数据库切换 → 立即重新加载
   - 固定最近 1 小时，**不受全局筛选影响**

**验证**：进入 Tab 图表 10s 刷新，离开停止，LIVE 动画正常

---

### 阶段 9：Tab 5 — 定价计算器

**目标**：模型搜索、Token 模拟计算、内联编辑、时间定价管理、汇率设置

**参考**：Java 版 `PricingTabView.java`（700 行）

**任务**：

1. **顶部工具栏（定价 Tab 专属，替换全局工具栏）**
   - 第一行：可搜索下拉（NSelect filterable）+ 清除按钮
   - 第二行：4 个 Token 输入（以千为单位，默认 输入1K/缓存读取70K/输出1K/缓存写入0K）+ 汇率输入框（默认 7.0）
   - Token 输入 200ms 防抖重算；汇率输入 500ms 防抖保存后触发全局刷新

2. **PricingCard.vue**
   - 模型名 + 编辑按钮
   - 计算费用（大号红色 4 位小数 = 顶部 Token × 定价 / 1,000,000）
   - 单价网格（4 行，颜色圆点 + 维度名 + 每百万单价）
   - "自定义"青色标签（有覆盖时）
   - 时间定价规则列表（时钟图标 + 标签/日期范围 + 编辑/删除按钮）
   - "添加时间定价"按钮

3. **PricingEditForm.vue**
   - 4 个单价输入框（预填当前定价）
   - 保存 → `pricing:set-override` → 全局级联刷新
   - 取消 → 退出编辑模式
   - 恢复默认（仅覆盖时显示）→ `pricing:remove-override`

4. **TimePricingDialog.vue**
   - 起始/结束日期（默认今天 ~ 今天+7天）
   - 4 个单价 + 标签
   - 添加/编辑两种模式

5. **卡片分组**
   - 已使用模型（始终展开）+ 未使用模型（默认折叠）
   - 按计算费用降序

**验证**：修改定价后所有 Tab 费用刷新，时间定价 CRUD 正常，汇率修改全局生效

---

### 阶段 10：级联刷新 + 错误处理 + 打包

**目标**：全局数据一致性、错误处理、应用打包

**任务**：

1. **级联刷新**（参考 Java 版 `onPricingChanged()`）
   ```
   定价/汇率修改
     → pricing:refresh（重载定价数据）
     → 重新 precompute（重算费用）
     → 通知 renderer 所有 Tab 刷新
   ```
   - 遵循 PRD 13.3 影响范围矩阵
   - 使用 Pinia action 统一触发

2. **版本号防竞态**（参考 Java 版 `queryVersion`）
   - 查询带版本号，结果返回时比对，过期则丢弃

3. **错误处理**
   - 数据库不存在 → 提示 "请选择数据库文件"
   - 数据库格式错误 → 错误对话框
   - 定价未初始化 → 费用显示 ¥0，不阻塞
   - 查询异常 → 错误对话框，不影响其他功能

4. **打包配置**
   - electron-builder（macOS dmg + Windows nsis）
   - better-sqlite3 native module 处理

**验证**：定价修改后切换 Tab 费用正确刷新，打包产物可安装运行

---

## 十一、依赖关系图

```
阶段 0：项目初始化
    │
    ▼
阶段 1：窗口管理 + 主布局
    │
    ▼
阶段 2：数据层（双数据库 + IPC）
    │
    ▼
阶段 3：定价引擎 + 预计算
    │
    ├──▶ 阶段 4：全局 UI ─── 同时依赖阶段 2
    │         │
    │         ▼
    │    阶段 5：按模型统计
    │         │
    │         ▼
    │    阶段 6：按供应商统计
    │
    ├──▶ 阶段 7：会话分析（依赖阶段 3）
    ├──▶ 阶段 8：实时 Token（依赖阶段 2）
    └──▶ 阶段 9：定价计算器（依赖阶段 3 + 4）
              │
              ▼
         阶段 10：集成 + 打包
```

**阶段 5/7/8/9 在阶段 3 和 4 完成后可并行开发**

---

## 十二、参考实现对照表

| Electron 模块 | Java 参考文件 | 行数 |
|---------------|-------------|------|
| window.ts | CcSwitchAnalyzer.java | 57 |
| external-db.ts | DatabaseService.java | 548 |
| app-db.ts | AppDatabaseService.java | 206 |
| pricing-engine.ts | PricingService.java | 203 |
| precompute.ts | MainController.precomputeCosts() | ~80 |
| Toolbar.vue | MainController.createToolbar() | ~60 |
| FilterBar.vue | FilterBar.java | 123 |
| SummaryBar.vue | SummaryBar.java | 56 |
| ByModel.vue + ModelCard.vue | ModelTabView.java | 574 |
| ByProvider.vue + ProviderCard.vue | ProviderTabView.java | 110 |
| SessionAnalysis.vue + SessionCard.vue | SessionTabView.java | 543 |
| RealtimeToken.vue + RealtimeAreaChart.vue | RealtimeTabView.java | 191 |
| PricingCalculator.vue + PricingCard.vue | PricingTabView.java | 700 |
| format.ts | UiUtils.java | 73 |
| constants.ts | Styles.java + AppConstants.java | 63 |
