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
      <div ref="wrapperRef" class="chart-wrapper">
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
        <div
          v-if="mode === 'byModel' && modelTipRows.length > 0"
          ref="tipRef"
          class="model-tip"
          :class="{ expandable: rankedModels.length > 5, dragging: tipDragging }"
          :style="tipStyle"
          title="拖动调整位置 · 双击空白处恢复自动避让"
          @mousedown="onTipDragStart"
          @click.capture="onTipClickCapture"
          @dblclick="onTipReset"
        >
          <div
            v-for="r in modelTipRows"
            :key="r.key"
            class="model-tip-row"
            :class="{ off: !modelVisible[r.key] }"
            :title="modelVisible[r.key] ? (groupByFamily ? '点击隐藏该家族' : '点击隐藏该模型') : (groupByFamily ? '点击显示该家族' : '点击显示该模型')"
            @click="modelVisible[r.key] = !modelVisible[r.key]"
            @mouseenter="hoveredModel = r.key"
            @mouseleave="hoveredModel = null"
          >
            <span class="model-tip-rank">{{ r.rank }}</span>
            <span class="model-tip-dot" :style="{ background: r.color }" />
            <span class="model-tip-model" :title="r.label">{{ r.label }}</span>
            <span class="model-tip-tokens">{{ formatNum(r.totalTokens) }}</span>
            <span class="model-tip-hit">缓存 {{ formatPercent(r.cacheHitRate) }}</span>
          </div>
          <button
            v-if="rankedModels.length > 5"
            class="model-tip-expand"
            :title="topModelLimit === 5 ? (groupByFamily ? '展开前十家族' : '展开前十模型') : '收起为前五'"
            @click.stop="topModelLimit = topModelLimit === 5 ? 10 : 5"
          >{{ topModelLimit === 5 ? '+' : '−' }}</button>
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
        <div class="toggle-bar-right">
          <button
            v-if="mode === 'byModel'"
            class="toggle-btn"
            :class="{ active: groupByFamily }"
            @click="groupByFamily = !groupByFamily"
          >家族</button>
          <button
            class="toggle-btn mode-toggle"
            :class="{ active: mode === 'byModel' }"
            @click="toggleMode"
          >{{ mode === 'overview' ? '模型' : '总览' }}</button>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch, onActivated, onDeactivated, onMounted, onUnmounted } from 'vue'
import { NSpin } from 'naive-ui'
import { useQueryStore } from '@/stores/query'
import { useFilterStore } from '@/stores/filter'
import { usePricingStore } from '@/stores/pricing'
import { platformAdapter } from '@/platform'
import TrendChart from '@/components/charts/TrendChart.vue'
import type { DailyTrendRow } from '@/types/database'
import type { PrecomputedResult } from '@/types/common'
import { formatNum, formatPercent } from '@/utils/format'
import {
  buildModelFamilyMap,
  familyLabel,
  getEffectiveFamilies,
  lookupModelFamily
} from '@/utils/family'
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
/** 按模型模式下的二次分组：按定价 family 聚合 */
const groupByFamily = ref(false)

// 进入"按模型对比"时,自动清空顶栏的模型/数据源筛选(对比本身就要求全量)
// 只清 modelId/providerId 两个字段,日期保留(避免影响用户的日期范围选择)
watch(mode, (m) => {
  if (m === 'byModel' && (filterStore.modelId || filterStore.providerId)) {
    filterStore.modelId = ''
    filterStore.providerId = ''
  }
  // 按模型对比下按天/按小时/按星期已隐藏,强制锁定为按天
  if (m === 'byModel') viewMode.value = 'daily'
  // 切回总览时关闭家族分组
  if (m === 'overview') groupByFamily.value = false
})

function toggleMode() {
  mode.value = mode.value === 'overview' ? 'byModel' : 'overview'
}

const effectiveFamilies = computed(() => getEffectiveFamilies(pricingStore.families))
const modelFamilyMap = computed(() =>
  buildModelFamilyMap(pricingStore.pricingData, effectiveFamilies.value)
)

function entityKeyForModel(model: string): string {
  return groupByFamily.value
    ? lookupModelFamily(model, modelFamilyMap.value)
    : model
}

function entityDisplayName(key: string): string {
  return groupByFamily.value
    ? familyLabel(key, effectiveFamilies.value)
    : key
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
  cancelTipDrag()
})

// ===== 定价查找表 =====
const pricingMap = computed(() => {
  return pricingStore.modelResolver
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

// ===== 按模型/家族对比:topN 实体 + 各自按 X 轴粒度的 token 序列 =====

const MODEL_SERIES_COLORS = [
  '--color-cost', '--color-green', '--color-blue', '--color-purple', '--color-orange',
  '--color-teal', '--color-indigo', '--color-dark-orange', '--color-cost', '--color-green',
]
const MODEL_TIP_COLORS = [
  '#e74c3c', '#0d8c6f', '#2980b9', '#8e44ad', '#f39c12',
  '#16a085', '#5b6abf', '#d35400', '#c0392b', '#27ae60',
]

/** tip 展示数量:默认前五,点 + 扩到前十 */
const topModelLimit = ref(5)

function rowTokens(r: DailyTrendRow): number {
  return r.inputTokens + r.outputTokens + r.cacheRead + r.cacheCreation
}

// 按 token 总量降序完整排名（模型 id 或 family id；随 precomputed / groupByFamily 重算）
const rankedModels = computed<string[]>(() => {
  const totals = new Map<string, number>()
  const pre = queryStore.precomputed
  if (pre?.dailyByModel) {
    for (const [model, rows] of Object.entries(pre.dailyByModel)) {
      let s = 0
      for (const r of rows) s += rowTokens(r)
      if (s <= 0) continue
      const key = entityKeyForModel(model)
      totals.set(key, (totals.get(key) || 0) + s)
    }
  }
  return [...totals.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([m]) => m)
})

const topModels = computed<string[]>(() =>
  rankedModels.value.slice(0, topModelLimit.value),
)

// 显隐状态 key = series.name（模型 id 或家族 label），与图表 dimming 对齐
const modelVisible = reactive<Record<string, boolean>>({})

const hoveredModel = ref<string | null>(null)

const dimmedModels = computed<string[]>(() => {
  if (!hoveredModel.value) return []
  return topModels.value
    .map(k => entityDisplayName(k))
    .filter(name => name !== hoveredModel.value && modelVisible[name] !== false)
})
watch(topModels, (keys) => {
  for (const k of keys) {
    const name = entityDisplayName(k)
    if (modelVisible[name] === undefined) modelVisible[name] = true
  }
}, { immediate: true })

watch(groupByFamily, () => {
  // 切换分组维度时重置显隐，避免模型 id 与家族 label 混用
  for (const k of Object.keys(modelVisible)) delete modelVisible[k]
  for (const k of topModels.value) modelVisible[entityDisplayName(k)] = true
  hoveredModel.value = null
  topModelLimit.value = 5
})

// 切回总览时重置为前五;切到 byModel 时全显并清空 hover
watch(mode, (m) => {
  if (m === 'overview') {
    topModelLimit.value = 5
  } else if (m === 'byModel') {
    for (const k of Object.keys(modelVisible)) modelVisible[k] = true
    hoveredModel.value = null
  }
})

function accumulateEntityTokens(
  dayMap: Record<string, DailyTrendRow[]>,
  entityKeys: string[],
  onRow: (entityKey: string, row: DailyTrendRow) => void
): void {
  const allow = new Set(entityKeys)
  for (const [model, rows] of Object.entries(dayMap)) {
    const ek = entityKeyForModel(model)
    if (!allow.has(ek)) continue
    for (const r of rows) onRow(ek, r)
  }
}

// 按 X 轴粒度聚合:每个实体一条 series(对齐到 display.dates)
const byModelSeries = computed<ModelSeries[]>(() => {
  const entities = topModels.value
  if (entities.length === 0) return []
  const pre = queryStore.precomputed
  const dates = display.dates
  const dayMap = pre?.dailyByModel || {}

  if (viewMode.value === 'weekday' && pre) {
    const f = filterStore.fromDate
    const toD = filterStore.toDate
    if (!f || !toD) return []
    const W = 7
    const acc = new Map<string, number[]>()
    for (const m of entities) acc.set(m, new Array(W).fill(0))
    const d = new Date(f.getFullYear(), f.getMonth(), f.getDate())
    const end = new Date(toD.getFullYear(), toD.getMonth(), toD.getDate())
    while (d <= end) {
      const dayKey = localDateStr(d)
      const w = (d.getDay() + 6) % 7
      accumulateEntityTokens(dayMap, entities, (ek, r) => {
        if (r.day === dayKey) acc.get(ek)![w] += rowTokens(r)
      })
      d.setDate(d.getDate() + 1)
    }
    return entities.map((m, i) => {
      const name = entityDisplayName(m)
      return {
        name,
        colorVar: MODEL_SERIES_COLORS[i] || MODEL_SERIES_COLORS[0],
        data: acc.get(m) || new Array(W).fill(0),
        visible: modelVisible[name] !== false
      }
    })
  }

  if ((viewMode.value === 'hourly' || (mode.value === 'byModel' && isSingleDay.value)) && hourlyRows.value.length > 0) {
    const allow = new Set(entities)
    const acc = new Map<string, number[]>()
    for (const m of entities) acc.set(m, new Array(24).fill(0))
    for (const r of hourlyRows.value) {
      const ek = entityKeyForModel(r.model)
      if (!allow.has(ek)) continue
      const hh = Number(r.day.split(':')[0])
      if (Number.isFinite(hh) && hh >= 0 && hh < 24) {
        acc.get(ek)![hh] += rowTokens(r)
      }
    }
    return entities.map((m, i) => {
      const name = entityDisplayName(m)
      return {
        name,
        colorVar: MODEL_SERIES_COLORS[i] || MODEL_SERIES_COLORS[0],
        data: acc.get(m) || new Array(24).fill(0),
        visible: modelVisible[name] !== false
      }
    })
  }

  if (dates.length === 0) return []
  const map = new Map<string, Map<string, number>>()
  for (const m of entities) map.set(m, new Map())
  accumulateEntityTokens(dayMap, entities, (ek, r) => {
    const inner = map.get(ek)!
    inner.set(r.day, (inner.get(r.day) || 0) + rowTokens(r))
  })
  return entities.map((m, i) => {
    const name = entityDisplayName(m)
    return {
      name,
      colorVar: MODEL_SERIES_COLORS[i] || MODEL_SERIES_COLORS[0],
      data: dates.map(d => map.get(m)?.get(d) || 0),
      visible: modelVisible[name] !== false
    }
  })
})

// ===== 按模型/家族对比:右上角汇总 tip =====
const modelTipRows = computed(() => {
  const pre = queryStore.precomputed
  if (!pre?.dailyByModel) return []
  const dayMap = pre.dailyByModel
  return topModels.value.map((ek, i) => {
    let input = 0, output = 0, cacheR = 0, cacheW = 0
    accumulateEntityTokens(dayMap, [ek], (_key, r) => {
      input += r.inputTokens
      output += r.outputTokens
      cacheR += r.cacheRead
      cacheW += r.cacheCreation
    })
    const total = input + output + cacheR + cacheW
    const hit = (input + cacheR + cacheW) > 0 ? cacheR / (input + cacheR + cacheW) : 0
    const name = entityDisplayName(ek)
    return {
      rank: i + 1,
      key: name,
      label: name,
      color: MODEL_TIP_COLORS[i] || MODEL_TIP_COLORS[0],
      totalTokens: total,
      cacheHitRate: hit
    }
  })
})

// ===== 右上角 tip:智能避让 + 拖拽 + 位置记忆 =====
// 问题:tip 固定右上角,而曲线高峰常偏右,浮层会挡住关键段(峰值/飙升段)。
// 策略:初始位置按曲线占用自动选更空的一侧;用户可拖动微调,位置按容器百分比记忆;
// 渲染时按当前容器与浮层尺寸即时 clamp(不改写原比例,避免切 Tab / resize 丢失位置);
// 双击空白处清除记忆,回到自动避让。

const TIP_POS_KEY = 'trend-analysis:model-tip-pos'
const tipRef = ref<HTMLElement | null>(null)
const wrapperRef = ref<HTMLElement | null>(null)
const tipDragging = ref(false)
/** null = 自动避让;否则为相对 chart-wrapper 的归一化比例 [0, 1] */
const tipPosPct = ref<{ xPct: number; yPct: number } | null>(loadTipPosPct())
/** 最近一次观测到的有效(>0)容器与浮层尺寸,隐藏(KeepAlive/最小化)时不归零 */
const tipLayoutSize = reactive({ wrapperW: 0, wrapperH: 0, tipW: 0, tipH: 0 })
let dragSession: { startX: number; startY: number; baseTop: number; baseLeft: number } | null = null
let dragArmed = false
let justDragged = false

function loadTipPosPct(): { xPct: number; yPct: number } | null {
  try {
    const raw = localStorage.getItem(TIP_POS_KEY)
    if (!raw) return null
    const p = JSON.parse(raw) as { xPct?: unknown; yPct?: unknown }
    if (
      typeof p.xPct === 'number' && typeof p.yPct === 'number' &&
      Number.isFinite(p.xPct) && Number.isFinite(p.yPct) &&
      p.xPct >= 0 && p.xPct <= 1 && p.yPct >= 0 && p.yPct <= 1
    ) {
      return { xPct: p.xPct, yPct: p.yPct }
    }
  } catch { /* 存储不可用或数据损坏,按无保存处理 */ }
  return null
}

/** 自动避让:比较左右两半列在图表顶部区域(归一化 > 0.7)的占用强度,选更空的一侧 */
const tipSide = computed<'left' | 'right'>(() => {
  const series = byModelSeries.value.filter(s => s.visible)
  const n = display.dates.length
  if (series.length === 0 || n < 2) return 'right'
  let globalMax = 0
  for (const s of series) {
    for (const v of s.data) if (v > globalMax) globalMax = v
  }
  if (globalMax <= 0) return 'right'
  const half = n / 2
  let leftWeight = 0
  let rightWeight = 0
  for (let x = 0; x < n; x++) {
    let colMax = 0
    for (const s of series) {
      const v = s.data[x] ?? 0
      if (v > colMax) colMax = v
    }
    const ratio = colMax / globalMax
    // 曲线爬进顶部约 30% 高度带才可能碰到浮层
    if (ratio > 0.7) {
      if (x < half) leftWeight += ratio
      else rightWeight += ratio
    }
  }
  if (leftWeight === 0 && rightWeight === 0) return 'right'
  return leftWeight < rightWeight ? 'left' : 'right'
})

/** 渲染位置:按记忆比例换算像素并即时 clamp,resize/展开后不溢出且可无损恢复 */
const tipStyle = computed(() => {
  const pct = tipPosPct.value
  if (pct) {
    if (tipLayoutSize.wrapperW > 0 && tipLayoutSize.wrapperH > 0) {
      const maxTop = Math.max(0, tipLayoutSize.wrapperH - tipLayoutSize.tipH)
      const maxLeft = Math.max(0, tipLayoutSize.wrapperW - tipLayoutSize.tipW)
      const top = Math.min(Math.max(pct.yPct * tipLayoutSize.wrapperH, 0), maxTop)
      const left = Math.min(Math.max(pct.xPct * tipLayoutSize.wrapperW, 0), maxLeft)
      return {
        top: `${Math.round(top)}px`,
        left: `${Math.round(left)}px`,
        right: 'auto'
      }
    }
    return {
      top: `${(pct.yPct * 100).toFixed(3)}%`,
      left: `${(pct.xPct * 100).toFixed(3)}%`,
      right: 'auto'
    }
  }
  if (tipSide.value === 'left') return { top: '36px', left: '64px', right: 'auto' }
  return undefined // 走 CSS 默认:top 36px / right 12px
})

function syncTipLayoutSize(): void {
  const wrapper = wrapperRef.value
  if (wrapper && wrapper.clientWidth > 0 && wrapper.clientHeight > 0) {
    tipLayoutSize.wrapperW = wrapper.clientWidth
    tipLayoutSize.wrapperH = wrapper.clientHeight
  }
  const tip = tipRef.value
  if (tip && tip.offsetWidth > 0 && tip.offsetHeight > 0) {
    tipLayoutSize.tipW = tip.offsetWidth
    tipLayoutSize.tipH = tip.offsetHeight
  }
}

function persistTipPos(): void {
  const pct = tipPosPct.value
  if (!pct) return
  try {
    localStorage.setItem(TIP_POS_KEY, JSON.stringify(pct))
  } catch { /* 存储不可用时静默,仅本次会话内生效 */ }
}

function cancelTipDrag(): void {
  if (!dragSession) return
  window.removeEventListener('mousemove', onTipDragMove)
  window.removeEventListener('mouseup', onTipDragEnd)
  dragSession = null
  dragArmed = false
  tipDragging.value = false
  justDragged = false
}

function onTipDragStart(e: MouseEvent): void {
  if (e.button !== 0) return
  if ((e.target as HTMLElement).closest('.model-tip-expand')) return
  const el = tipRef.value
  const wrapper = wrapperRef.value
  if (!el || !wrapper || wrapper.clientWidth <= 0 || wrapper.clientHeight <= 0) return
  syncTipLayoutSize()
  const rect = el.getBoundingClientRect()
  const parentRect = wrapper.getBoundingClientRect()
  dragSession = {
    startX: e.clientX,
    startY: e.clientY,
    baseTop: rect.top - parentRect.top,
    baseLeft: rect.left - parentRect.left
  }
  dragArmed = true
  window.addEventListener('mousemove', onTipDragMove)
  window.addEventListener('mouseup', onTipDragEnd)
}

function onTipDragMove(e: MouseEvent): void {
  if (!dragSession) return
  const wrapper = wrapperRef.value
  const el = tipRef.value
  if (!wrapper || !el || wrapper.clientWidth <= 0 || wrapper.clientHeight <= 0) return
  const dx = e.clientX - dragSession.startX
  const dy = e.clientY - dragSession.startY
  // 位移超过阈值才进入拖拽,避免吞掉行上原有的点击交互
  if (dragArmed && Math.abs(dx) < 4 && Math.abs(dy) < 4) return
  if (dragArmed) {
    dragArmed = false
    justDragged = true
    tipDragging.value = true
  }
  const maxTop = Math.max(0, wrapper.clientHeight - el.offsetHeight)
  const maxLeft = Math.max(0, wrapper.clientWidth - el.offsetWidth)
  const top = Math.min(Math.max(dragSession.baseTop + dy, 0), maxTop)
  const left = Math.min(Math.max(dragSession.baseLeft + dx, 0), maxLeft)
  tipPosPct.value = {
    xPct: Math.min(1, Math.max(0, left / wrapper.clientWidth)),
    yPct: Math.min(1, Math.max(0, top / wrapper.clientHeight))
  }
}

function onTipDragEnd(): void {
  window.removeEventListener('mousemove', onTipDragMove)
  window.removeEventListener('mouseup', onTipDragEnd)
  if (tipDragging.value) persistTipPos()
  dragSession = null
  dragArmed = false
  tipDragging.value = false
  // click 事件在 mouseup 后同步派发,由 click.capture 消费;兜底定时清
  setTimeout(() => { justDragged = false }, 0)
}

/** 拖拽结束后拦截紧随的 click,避免误触行的显隐切换 */
function onTipClickCapture(e: MouseEvent): void {
  if (justDragged) {
    e.stopPropagation()
    e.preventDefault()
    justDragged = false
  }
}

/** 双击空白处(非行/非按钮):清除记忆,回到自动避让 */
function onTipReset(e: MouseEvent): void {
  if ((e.target as HTMLElement).closest('.model-tip-row, .model-tip-expand')) return
  tipPosPct.value = null
  try { localStorage.removeItem(TIP_POS_KEY) } catch { /* 忽略 */ }
}

let tipResizeObserver: ResizeObserver | null = null

function rebindTipObserver(): void {
  if (!tipResizeObserver) return
  tipResizeObserver.disconnect()
  if (wrapperRef.value) tipResizeObserver.observe(wrapperRef.value)
  if (tipRef.value) tipResizeObserver.observe(tipRef.value)
  syncTipLayoutSize()
}

onMounted(() => {
  tipResizeObserver = new ResizeObserver(syncTipLayoutSize)
  rebindTipObserver()
})

onUnmounted(() => {
  tipResizeObserver?.disconnect()
  tipResizeObserver = null
  cancelTipDrag()
})

// 浮层与容器都是 v-if 延迟渲染(loading/空态/overview 无此节点),变化后重挂观察器并同步尺寸
watch([wrapperRef, tipRef], rebindTipObserver, { flush: 'post' })

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
  background: transparent;
  border-radius: 0;
  border: 0;
  padding: 4px 0 0;
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
  background: color-mix(in srgb, var(--bg-card) 92%, transparent);
  border-radius: 8px;
  font-size: 10px;
  line-height: 1.5;
  color: var(--text-primary);
  pointer-events: auto;
  max-width: 240px;
  user-select: none;
  cursor: grab;
}
.model-tip.dragging {
  cursor: grabbing;
  opacity: 0.92;
}
.model-tip.expandable {
  padding-bottom: 18px;
}
.model-tip-expand {
  position: absolute;
  right: 4px;
  bottom: 2px;
  width: 14px;
  height: 14px;
  padding: 0;
  border: 1px solid rgba(255, 255, 255, 0.22);
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.06);
  color: #c8c8d8;
  font-size: 11px;
  line-height: 12px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}
.model-tip-expand:hover {
  background: rgba(255, 255, 255, 0.14);
  color: #fff;
  border-color: rgba(255, 255, 255, 0.35);
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

.toggle-bar-right {
  margin-left: auto;
  align-items: center;
}

.mode-toggle {
  margin-left: 0;
}

.toggle-btn {
  padding: 3px 10px;
  font-size: 11px;
  font-weight: 500;
  border: 0.5px solid var(--border-main);
  border-radius: var(--chip-radius);
  background: transparent;
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
