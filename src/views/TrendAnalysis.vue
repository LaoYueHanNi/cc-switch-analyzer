<template>
  <div class="trend-analysis">
    <!-- 加载状态（仅首次加载显示） -->
    <div v-if="(queryStore.loading || (hourlyLoading && viewMode !== 'weekday')) && display.dates.length === 0" class="tab-loading">
      <n-spin size="medium" />
      <p>正在查询...</p>
    </div>

    <!-- 空数据 -->
    <div v-else-if="display.dates.length === 0 && !queryStore.loading && !(hourlyLoading && viewMode !== 'weekday')" class="tab-empty">
      <p>暂无数据，请调整筛选条件</p>
    </div>

    <!-- 图表 + 按钮组 -->
    <template v-else>
      <div class="chart-wrapper">
        <TrendChart
          :dates="display.dates"
          :total-cost-data="display.cost"
          :total-token-data="display.token"
          :input-data="display.input"
          :output-data="display.output"
          :cache-read-data="display.cacheR"
          :cache-creation-data="display.cacheW"
          :visible-series="visibleSeries"
          :mode="mode"
          :model-series="byModelSeries"
          :dimmed-models="dimmedModels"
        />
        <div v-if="mode === 'byModel' && modelTipRows.length > 0" class="model-tip">
          <div
            v-for="r in modelTipRows"
            :key="r.model"
            class="model-tip-row"
            :class="{ off: !modelVisible[r.model] }"
            :title="modelVisible[r.model] ? '点击隐藏该模型' : '点击显示该模型'"
            @click="modelVisible[r.model] = !modelVisible[r.model]"
            @mouseenter="hoveredModel = r.model"
            @mouseleave="hoveredModel = null"
          >
            <span class="model-tip-rank">{{ r.rank }}</span>
            <span class="model-tip-dot" :style="{ background: r.color }" />
            <span class="model-tip-model" :title="r.model">{{ r.model }}</span>
            <span class="model-tip-tokens">{{ formatNum(r.totalTokens) }}</span>
            <span class="model-tip-hit">缓存 {{ formatPercent(r.cacheHitRate) }}</span>
          </div>
        </div>
      </div>

      <div class="toggle-bar">
        <div class="toggle-bar-left" v-if="hourlyEnabled && mode === 'overview'">
          <button class="toggle-btn" :class="{ active: viewMode === 'daily' }" @click="viewMode = 'daily'">按天</button>
          <button v-if="hourlyEnabled" class="toggle-btn" :class="{ active: viewMode === 'hourly' }" @click="viewMode = 'hourly'">按小时</button>
          <button v-if="weekdayEnabled" class="toggle-btn" :class="{ active: viewMode === 'weekday' }" @click="viewMode = 'weekday'">按星期</button>
        </div>
        <div v-if="mode === 'overview'" class="toggle-bar-center">
          <button
            class="toggle-btn btn-cost"
            :class="{ active: visibleSeries.cost }"
            @click="visibleSeries.cost = !visibleSeries.cost"
          >总费用</button>
          <button
            class="toggle-btn btn-token"
            :class="{ active: visibleSeries.total }"
            @click="visibleSeries.total = !visibleSeries.total"
          >总Token</button>
          <button
            class="toggle-btn btn-detail"
            :class="{ active: visibleSeries.detail }"
            @click="visibleSeries.detail = !visibleSeries.detail"
          >Token明细</button>
        </div>
        <button
          class="toggle-btn mode-toggle"
          :class="{ active: mode === 'byModel' }"
          @click="toggleMode"
        >{{ mode === 'overview' ? '模型' : '总览' }}</button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch, onActivated, onDeactivated } from 'vue'
import { NSpin } from 'naive-ui'
import { useQueryStore } from '@/stores/query'
import { useFilterStore } from '@/stores/filter'
import { usePricingStore } from '@/stores/pricing'
import { platformAdapter } from '@/platform'
import TrendChart from '@/components/charts/TrendChart.vue'
import type { DailyTrendRow } from '@/types/database'
import type { PricingData } from '@/types/pricing'
import type { PrecomputedResult } from '@/types/common'
import { formatNum, formatPercent } from '@/utils/format'
import type { ModelSeries } from '@/components/charts/TrendChart.vue'

defineOptions({ name: 'TrendAnalysis' })

const queryStore = useQueryStore()
const filterStore = useFilterStore()
const pricingStore = usePricingStore()

const visibleSeries = reactive({ cost: true, total: true, detail: false })

// ===== 视图模式（radio 互斥） =====
type ViewMode = 'daily' | 'hourly' | 'weekday'
const viewMode = ref<ViewMode>('daily')

// ===== 图表模式:总览 vs 按模型对比 =====
type ChartMode = 'overview' | 'byModel'
const mode = ref<ChartMode>('overview')

// 进入"按模型对比"时,自动清空顶栏的模型/供应商筛选(对比本身就要求全量)
// 只清 modelId/providerId 两个字段,日期保留(避免影响用户的日期范围选择)
watch(mode, (m) => {
  if (m === 'byModel' && (filterStore.modelId || filterStore.providerId)) {
    filterStore.modelId = ''
    filterStore.providerId = ''
  }
  // 按模型对比下按天/按小时/按星期已隐藏,强制锁定为按天
  if (m === 'byModel') viewMode.value = 'daily'
})

function toggleMode() {
  mode.value = mode.value === 'overview' ? 'byModel' : 'overview'
}

// ===== 单日检测（用本地时间比较，避免 toISOString 的 UTC 偏移） =====
function localDateStr(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

const isSingleDay = computed(() => {
  const f = filterStore.fromDate
  const t = filterStore.toDate
  if (!f || !t) return false
  return localDateStr(f) === localDateStr(t)
})

const dayCount = computed(() => {
  const f = filterStore.fromDate
  const t = filterStore.toDate
  if (!f || !t) return 0
  const msDay = 86_400_000
  const fromMs = new Date(f.getFullYear(), f.getMonth(), f.getDate()).getTime()
  const toMs = new Date(t.getFullYear(), t.getMonth(), t.getDate()).getTime()
  return Math.round((toMs - fromMs) / msDay) + 1
})

const hourlyEnabled = computed(() => dayCount.value > 1 && !isSingleDay.value)
const weekdayEnabled = computed(() => dayCount.value > 7)
const needsHourly = computed(() => isSingleDay.value || viewMode.value === 'hourly')

// 日期范围缩小时自动降级到 daily
watch([hourlyEnabled, weekdayEnabled], () => {
  if (viewMode.value === 'hourly' && !hourlyEnabled.value) viewMode.value = 'daily'
  if (viewMode.value === 'weekday' && !weekdayEnabled.value) viewMode.value = 'daily'
})

// ===== 小时数据（竞态防护） =====
const hourlyRows = ref<DailyTrendRow[]>([])
const hourlyLoading = ref(false)
let fetchId = 0

async function fetchHourly(): Promise<void> {
  const id = ++fetchId
  hourlyLoading.value = true
  try {
    const result = await platformAdapter.queryHourlyTrend(filterStore.filterParams)
    if (id !== fetchId) return
    hourlyRows.value = result
  } catch {
    if (id !== fetchId) return
    hourlyRows.value = []
  } finally {
    if (id === fetchId) hourlyLoading.value = false
  }
}

// 被 KeepAlive 缓存后不在前台时，暂缓请求，避免后台 Tab 持续拉取小时数据
const isActive = ref(true)
let pendingHourlyFetch = false

function tryFetchHourly(): void {
  if (isActive.value) fetchHourly()
  else pendingHourlyFetch = true
}

watch(needsHourly, (need) => {
  if (need) tryFetchHourly()
  else hourlyRows.value = []
}, { immediate: true })
watch(() => filterStore.filterParams, () => { if (needsHourly.value) tryFetchHourly() }, { deep: true })
watch(() => queryStore.queryVersion, () => { if (needsHourly.value) tryFetchHourly() })

onActivated(() => {
  isActive.value = true
  if (pendingHourlyFetch) {
    pendingHourlyFetch = false
    if (needsHourly.value) fetchHourly()
  }
})
onDeactivated(() => {
  isActive.value = false
})

// ===== 定价查找表 =====
const pricingMap = computed(() => {
  const map = new Map<string, PricingData>()
  for (const p of pricingStore.pricingData) {
    map.set(p.modelId, p)
    for (const alias of p.aliases || []) map.set(alias, p)
  }
  return map
})

// 从预计算结果推算每个模型的加权费率（基于 tier-aware 分解）
const blendedRates = computed(() => {
  const pre = queryStore.precomputed
  if (!pre?.modelCostBreakdown) return new Map<string, { input: number; output: number; cacheRead: number; cacheCreation: number }>()
  const rates = new Map<string, { input: number; output: number; cacheRead: number; cacheCreation: number }>()
  for (const mb of queryStore.modelBreakdown) {
    const bd = pre.modelCostBreakdown[mb.model]
    if (!bd) continue
    const M = 1_000_000
    rates.set(mb.model, {
      input: mb.inputTokens > 0 ? bd[0] * M / mb.inputTokens : (pricingMap.value.get(mb.model)?.inputCostPerMillion || 0),
      output: mb.outputTokens > 0 ? bd[1] * M / mb.outputTokens : (pricingMap.value.get(mb.model)?.outputCostPerMillion || 0),
      cacheRead: mb.cacheRead > 0 ? bd[2] * M / mb.cacheRead : (pricingMap.value.get(mb.model)?.cacheReadCostPerMillion || 0),
      cacheCreation: mb.cacheCreation > 0 ? bd[3] * M / mb.cacheCreation : (pricingMap.value.get(mb.model)?.cacheCreationCostPerMillion || 0),
    })
  }
  return rates
})

function getCostForRow(row: DailyTrendRow): number {
  const pricing = pricingMap.value.get(row.model)
  if (!pricing) return 0
  const blended = blendedRates.value.get(row.model)
  if (blended) {
    const M = 1_000_000
    return (
      row.inputTokens * blended.input / M +
      row.outputTokens * blended.output / M +
      row.cacheRead * blended.cacheRead / M +
      row.cacheCreation * blended.cacheCreation / M
    )
  }
  const M = 1_000_000
  return (
    row.inputTokens * (pricing.inputCostPerMillion || 0) / M +
    row.outputTokens * (pricing.outputCostPerMillion || 0) / M +
    row.cacheRead * (pricing.cacheReadCostPerMillion || 0) / M +
    row.cacheCreation * (pricing.cacheCreationCostPerMillion || 0) / M
  )
}

// ===== 数据计算 =====
const ALL_HOURS = Array.from({ length: 24 }, (_, i) => String(i).padStart(2, '0') + ':00')
const WEEKDAY_LABELS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日']

function generateDateRange(from: Date, to: Date): string[] {
  const dates: string[] = []
  const d = new Date(from.getFullYear(), from.getMonth(), from.getDate())
  const end = new Date(to.getFullYear(), to.getMonth(), to.getDate())
  while (d <= end) {
    dates.push(localDateStr(d))
    d.setDate(d.getDate() + 1)
  }
  return dates
}

function aggregateHourlyRows(getter: (row: DailyTrendRow) => number): number[] {
  const map = new Map<string, number>()
  for (const row of hourlyRows.value) map.set(row.day, (map.get(row.day) || 0) + getter(row))
  return ALL_HOURS.map(h => map.get(h) || 0)
}

function aggregateWeekday(pre: PrecomputedResult, from: Date, to: Date): {
  dates: string[]; cost: number[]; token: number[];
  input: number[]; output: number[]; cacheR: number[]; cacheW: number[]
} {
  const W = 7
  const costB = new Array(W).fill(0)
  const inputB = new Array(W).fill(0)
  const outputB = new Array(W).fill(0)
  const cacheRB = new Array(W).fill(0)
  const cacheWB = new Array(W).fill(0)

  const d = new Date(from.getFullYear(), from.getMonth(), from.getDate())
  const end = new Date(to.getFullYear(), to.getMonth(), to.getDate())
  while (d <= end) {
    const key = localDateStr(d)
    const w = (d.getDay() + 6) % 7 // Mon=0 .. Sun=6
    costB[w] += pre.dayCostMap?.[key] || 0
    inputB[w] += pre.dayInputTokens?.[key] || 0
    outputB[w] += pre.dayOutputTokens?.[key] || 0
    cacheRB[w] += pre.dayCacheRead?.[key] || 0
    cacheWB[w] += pre.dayCacheCreation?.[key] || 0
    d.setDate(d.getDate() + 1)
  }

  return {
    dates: WEEKDAY_LABELS,
    cost: costB,
    token: inputB.map((v, i) => v + outputB[i] + cacheRB[i] + cacheWB[i]),
    input: inputB, output: outputB, cacheR: cacheRB, cacheW: cacheWB
  }
}

interface SeriesData {
  dates: string[]; cost: number[]; token: number[];
  input: number[]; output: number[]; cacheR: number[]; cacheW: number[]
}

const allSeriesData = computed<SeriesData | null>(() => {
  const f = filterStore.fromDate
  const t = filterStore.toDate

  // 重置后日期为 null 时，回退到全量数据的日期范围
  const fromDate = f || (filterStore.dateRangeMin != null ? new Date(filterStore.dateRangeMin * 1000) : null)
  const toDate = t || (filterStore.dateRangeMax != null ? new Date(filterStore.dateRangeMax * 1000) : null)

  // 星期视图：从 precomputed day*Map 按星期聚合
  if (viewMode.value === 'weekday' && fromDate && toDate) {
    const pre = queryStore.precomputed
    if (pre) return aggregateWeekday(pre, fromDate!, toDate!)
  }

  // 小时视图：复用后端 queryHourlyTrend（跨天聚合到 24 小时桶）
  if ((viewMode.value === 'hourly' || isSingleDay.value) && hourlyRows.value.length > 0) {
    return {
      dates: ALL_HOURS,
      cost: aggregateHourlyRows(r => getCostForRow(r)),
      token: aggregateHourlyRows(r => r.inputTokens + r.outputTokens + r.cacheRead + r.cacheCreation),
      input: aggregateHourlyRows(r => r.inputTokens),
      output: aggregateHourlyRows(r => r.outputTokens),
      cacheR: aggregateHourlyRows(r => r.cacheRead),
      cacheW: aggregateHourlyRows(r => r.cacheCreation)
    }
  }

  // 每日视图（默认）：基于日期范围的完整序列
  if (!fromDate || !toDate) return null
  const dateList = generateDateRange(fromDate, toDate)
  const pre = queryStore.precomputed
  return {
    dates: dateList,
    cost: dateList.map(d => pre?.dayCostMap?.[d] || 0),
    token: dateList.map(d => pre
      ? (pre.dayInputTokens?.[d] || 0) + (pre.dayOutputTokens?.[d] || 0) + (pre.dayCacheRead?.[d] || 0) + (pre.dayCacheCreation?.[d] || 0)
      : 0),
    input: dateList.map(d => pre?.dayInputTokens?.[d] || 0),
    output: dateList.map(d => pre?.dayOutputTokens?.[d] || 0),
    cacheR: dateList.map(d => pre?.dayCacheRead?.[d] || 0),
    cacheW: dateList.map(d => pre?.dayCacheCreation?.[d] || 0)
  }
})

// ===== 按模型对比:top5 模型 + 各自按 X 轴粒度的 token 序列 =====

function rowTokens(r: DailyTrendRow): number {
  return r.inputTokens + r.outputTokens + r.cacheRead + r.cacheCreation
}

// 按 token 总量降序取 top5(覆盖按天/按小时/按星期三种粒度的数据源)
const topModels = computed<string[]>(() => {
  const totals = new Map<string, number>()
  const pre = queryStore.precomputed
  // 统一用 dailyByModel(daily/weekday/hourly 三种粒度下都涵盖完整筛选范围,不会重复计数)
  if (pre?.dailyByModel) {
    for (const [model, rows] of Object.entries(pre.dailyByModel)) {
      let s = 0
      for (const r of rows) s += rowTokens(r)
      if (s > 0) totals.set(model, s)
    }
  }
  return [...totals.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5)
    .map(([m]) => m)
})

// 模型显隐状态(由 tip 行点击切换;true=显示)
const modelVisible = reactive<Record<string, boolean>>({})

// 鼠标 hover 的模型(tip 行 mouseenter/leave 切换;null=无 hover)
const hoveredModel = ref<string | null>(null)

// 压暗列表:hover 某 model 时,把其他所有"显示中"的 model 加进去
const dimmedModels = computed<string[]>(() => {
  if (!hoveredModel.value) return []
  return topModels.value.filter(m => m !== hoveredModel.value && modelVisible[m] !== false)
})
watch(topModels, (models) => {
  // 只新增不删除:保留用户的历史显隐选择,避免 model 暂时离开 top5 后选择被清零
  // 渲染时按 topModels 过滤;modelVisible[m] 未定义(undefined)等同 true(因为 !== false 包含 undefined)
  for (const m of models) if (modelVisible[m] === undefined) modelVisible[m] = true
}, { immediate: true })

// 切到 byModel 时全显(避免上次点关后再次进入时全空)
// 同时清空 hover 状态,避免跨 mode + topModels 变化时首屏全 dim
watch(mode, (m) => {
  if (m === 'byModel') {
    for (const k of Object.keys(modelVisible)) modelVisible[k] = true
    hoveredModel.value = null
  }
})

// 按 X 轴粒度聚合:每个 model 一条 series(对齐到 display.dates)
const byModelSeries = computed<ModelSeries[]>(() => {
  const models = topModels.value
  if (models.length === 0) return []
  // 用 day -> { model -> token } map
  const map = new Map<string, Map<string, number>>()
  const pre = queryStore.precomputed
  const dates = display.dates

  if (viewMode.value === 'weekday' && pre) {
    // weekday 粒度:dailyByModel 按 weekday(0..6,Mon..Sun)聚合
    const f = filterStore.fromDate
    const toD = filterStore.toDate
    if (!f || !toD) return []
    const W = 7
    const acc = new Map<string, number[]>() // model -> 7 桶
    for (const m of models) acc.set(m, new Array(W).fill(0))
    const d = new Date(f.getFullYear(), f.getMonth(), f.getDate())
    const end = new Date(toD.getFullYear(), toD.getMonth(), toD.getDate())
    while (d <= end) {
      const key = localDateStr(d)
      const w = (d.getDay() + 6) % 7
      const dayMap = pre.dailyByModel || {}
      for (const m of models) {
        const rows = dayMap[m] || []
        for (const r of rows) {
          if (r.day === key) acc.get(m)![w] += rowTokens(r)
        }
      }
      d.setDate(d.getDate() + 1)
    }
    return models.map((m, i) => ({
      name: m,
      colorVar: ['--color-cost', '--color-green', '--color-blue', '--color-purple', '--color-orange'][i] || '--color-cost',
      data: acc.get(m) || new Array(W).fill(0),
      visible: modelVisible[m] !== false
    }))
  }

  if ((viewMode.value === 'hourly' || (mode.value === 'byModel' && isSingleDay.value)) && hourlyRows.value.length > 0) {
    // hourly 粒度:24 小时桶(byModel 模式下单日筛选也走这条,避免 5 条线在 1 个点重合)
    // 守卫 hourlyRows 已就绪(对齐 allSeriesData line 309 的守卫),否则 fallback 到 daily 分支
    const acc = new Map<string, number[]>()
    for (const m of models) acc.set(m, new Array(24).fill(0))
    for (const r of hourlyRows.value) {
      if (!models.includes(r.model)) continue
      const hh = Number(r.day.split(':')[0])
      if (Number.isFinite(hh) && hh >= 0 && hh < 24) {
        acc.get(r.model)![hh] += rowTokens(r)
      }
    }
    return models.map((m, i) => ({
      name: m,
      colorVar: ['--color-cost', '--color-green', '--color-blue', '--color-purple', '--color-orange'][i] || '--color-cost',
      data: acc.get(m) || new Array(24).fill(0),
      visible: modelVisible[m] !== false
    }))
  }

  // daily 粒度:对齐到 display.dates(已经是日期范围序列)
  // 守卫下移到这里:hourly/weekday 分支用固定 24/7 桶,不读 dates
  if (dates.length === 0) return []
  for (const m of models) map.set(m, new Map())
  const dayMap = pre?.dailyByModel || {}
  for (const m of models) {
    const rows = dayMap[m] || []
    const inner = map.get(m)!
    for (const r of rows) inner.set(r.day, (inner.get(r.day) || 0) + rowTokens(r))
  }
  return models.map((m, i) => ({
    name: m,
    colorVar: ['--color-cost', '--color-green', '--color-blue', '--color-purple', '--color-orange'][i] || '--color-cost',
    data: dates.map(d => map.get(m)?.get(d) || 0),
    visible: modelVisible[m] !== false
  }))
})

// ===== 按模型对比:右上角汇总 tip =====
const MODEL_TIP_COLORS = ['#e74c3c', '#0d8c6f', '#2980b9', '#8e44ad', '#f39c12']

const modelTipRows = computed(() => {
  const pre = queryStore.precomputed
  if (!pre?.dailyByModel) return []
  const rows = topModels.value.map((m, i) => {
    const dayRows = pre.dailyByModel[m] || []
    let input = 0, output = 0, cacheR = 0, cacheW = 0
    for (const r of dayRows) {
      input += r.inputTokens
      output += r.outputTokens
      cacheR += r.cacheRead
      cacheW += r.cacheCreation
    }
    const total = input + output + cacheR + cacheW
    // 缓存命中率 = cacheRead / (input + cacheRead + cacheWrite)
    const hit = (input + cacheR + cacheW) > 0 ? cacheR / (input + cacheR + cacheW) : 0
    return {
      rank: i + 1,
      model: m,
      color: MODEL_TIP_COLORS[i] || MODEL_TIP_COLORS[0],
      totalTokens: total,
      cacheHitRate: hit
    }
  })
  return rows
})

// ===== 持久化有效数据，刷新期间保持图表显示 =====
const display = reactive({
  dates: [] as string[], cost: [] as number[], token: [] as number[],
  input: [] as number[], output: [] as number[], cacheR: [] as number[], cacheW: [] as number[]
})

watch(allSeriesData, (v) => {
  if (v && v.dates.length > 0) {
    Object.assign(display, v)
  } else if (!queryStore.loading && !hourlyLoading.value) {
    display.dates = []; display.cost = []; display.token = []
    display.input = []; display.output = []; display.cacheR = []; display.cacheW = []
  }
}, { immediate: true })
</script>

<style scoped>
.trend-analysis {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.chart-wrapper {
  flex: 1;
  min-height: 0;
  background: var(--bg-card);
  border-radius: 8px;
  border: 1px solid var(--border-main);
  padding: 12px;
  position: relative;
}

/* 按模型对比:图表右上角汇总 tip(排名 + 模型 + Token + 缓存命中率) */
.model-tip {
  position: absolute;
  top: 36px;
  right: 12px;
  z-index: 5;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 6px 8px;
  background: rgba(20, 20, 40, 0.78);
  border-radius: 4px;
  font-size: 10px;
  line-height: 1.5;
  color: #e6e6f0;
  pointer-events: auto;
  max-width: 240px;
  user-select: none;
}
.model-tip-row {
  display: grid;
  grid-template-columns: 12px 8px 1fr auto auto;
  align-items: center;
  gap: 5px;
  padding: 0 2px;
  border-radius: 3px;
  cursor: pointer;
  transition: background 0.12s, opacity 0.12s;
}
.model-tip-row:hover { background: rgba(255, 255, 255, 0.08); }
.model-tip-row.off { opacity: 0.4; }
.model-tip-row.off:hover { opacity: 0.7; }
.model-tip-rank {
  color: #9090a8;
  font-size: 9px;
  text-align: right;
}
.model-tip-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
.model-tip-model {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}
.model-tip-tokens {
  color: var(--color-green);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.model-tip-hit {
  color: var(--color-teal);
  font-variant-numeric: tabular-nums;
}

/* (旧的 chip 样式已删除) */

.toggle-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  position: relative;
}

.toggle-bar-left,
.toggle-bar-right,
.toggle-bar-center {
  display: flex;
  gap: 4px;
}

/* 系列可见性按钮组:绝对居中,不受 left/mode-toggle 宽度影响 */
.toggle-bar-center {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
}

.mode-toggle {
  margin-left: auto;
}

.toggle-btn {
  padding: 3px 10px;
  font-size: 11px;
  font-weight: 500;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: var(--bg-card);
  color: var(--text-muted);
  cursor: pointer;
  transition: all var(--transition-speed);
  line-height: 1.4;
}

.toggle-btn:hover {
  border-color: var(--color-blue);
  color: var(--text-primary);
}

.toggle-btn.active {
  border-color: var(--color-blue);
  background: var(--color-blue-bg);
  color: var(--color-blue);
}

.btn-cost.active {
  border-color: var(--color-cost);
  background: rgba(231, 76, 60, 0.08);
  color: var(--color-cost);
}

.btn-token.active {
  border-color: var(--color-green);
  background: rgba(22, 160, 133, 0.08);
  color: var(--color-green);
}

.btn-detail.active {
  border-color: var(--color-purple);
  background: var(--color-purple-bg);
  color: var(--color-purple);
}
</style>
