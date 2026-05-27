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
        />
      </div>

      <div class="toggle-bar">
        <div class="toggle-bar-left" v-if="hourlyEnabled || weekdayEnabled">
          <button class="toggle-btn" :class="{ active: viewMode === 'daily' }" @click="viewMode = 'daily'">按天</button>
          <button v-if="hourlyEnabled" class="toggle-btn" :class="{ active: viewMode === 'hourly' }" @click="viewMode = 'hourly'">按小时</button>
          <button v-if="weekdayEnabled" class="toggle-btn" :class="{ active: viewMode === 'weekday' }" @click="viewMode = 'weekday'">按星期</button>
        </div>
        <div class="toggle-bar-right">
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
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { NSpin } from 'naive-ui'
import { useQueryStore } from '@/stores/query'
import { useFilterStore } from '@/stores/filter'
import { usePricingStore } from '@/stores/pricing'
import { platformAdapter } from '@/platform'
import TrendChart from '@/components/charts/TrendChart.vue'
import type { DailyTrendRow } from '@/types/database'
import type { PricingData } from '@/types/pricing'
import type { PrecomputedResult } from '@/types/common'

defineOptions({ name: 'TrendAnalysis' })

const queryStore = useQueryStore()
const filterStore = useFilterStore()
const pricingStore = usePricingStore()

const visibleSeries = reactive({ cost: true, total: true, detail: false })

// ===== 视图模式（radio 互斥） =====
type ViewMode = 'daily' | 'hourly' | 'weekday'
const viewMode = ref<ViewMode>('daily')

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

watch(needsHourly, (need) => {
  if (need) fetchHourly()
  else hourlyRows.value = []
}, { immediate: true })
watch(() => filterStore.filterParams, () => { if (needsHourly.value) fetchHourly() }, { deep: true })
watch(() => queryStore.queryVersion, () => { if (needsHourly.value) fetchHourly() })

// ===== 定价查找表 =====
const pricingMap = computed(() => {
  const map = new Map<string, PricingData>()
  for (const p of pricingStore.pricingData) {
    map.set(p.modelId, p)
    for (const alias of p.aliases || []) map.set(alias, p)
  }
  return map
})

function getCostForRow(row: DailyTrendRow): number {
  const pricing = pricingMap.value.get(row.model)
  if (!pricing) return 0
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
}

.toggle-bar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.toggle-bar-left,
.toggle-bar-right {
  display: flex;
  gap: 4px;
}

.toggle-bar-right {
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
