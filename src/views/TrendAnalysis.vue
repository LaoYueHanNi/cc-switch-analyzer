<template>
  <div class="trend-analysis">
    <!-- 加载状态（仅首次加载显示） -->
    <div v-if="(queryStore.loading || hourlyLoading) && display.dates.length === 0" class="tab-loading">
      <n-spin size="medium" />
      <p>正在查询...</p>
    </div>

    <!-- 空数据 -->
    <div v-else-if="display.dates.length === 0 && !isSingleDay && !queryStore.loading" class="tab-empty">
      <p>暂无数据，请调整筛选条件</p>
    </div>
    <div v-else-if="isSingleDay && hourlyRows.length === 0 && !hourlyLoading && !queryStore.loading" class="tab-empty">
      <p>该日暂无数据</p>
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
        <button
          class="toggle-btn btn-cost"
          :class="{ active: visibleSeries.cost }"
          @click="visibleSeries.cost = !visibleSeries.cost"
        >
          总费用
        </button>
        <button
          class="toggle-btn btn-token"
          :class="{ active: visibleSeries.total }"
          @click="visibleSeries.total = !visibleSeries.total"
        >
          总Token
        </button>
        <button
          class="toggle-btn btn-detail"
          :class="{ active: visibleSeries.detail }"
          @click="visibleSeries.detail = !visibleSeries.detail"
        >
          Token明细
        </button>
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

defineOptions({ name: 'TrendAnalysis' })

const queryStore = useQueryStore()
const filterStore = useFilterStore()
const pricingStore = usePricingStore()

const visibleSeries = reactive({ cost: true, total: true, detail: false })

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

watch(isSingleDay, (single) => {
  if (single) fetchHourly()
  else hourlyRows.value = []
}, { immediate: true })
watch(() => filterStore.filterParams, () => { if (isSingleDay.value) fetchHourly() }, { deep: true })
watch(() => queryStore.queryVersion, () => { if (isSingleDay.value) fetchHourly() })

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

function aggregateHourly(getter: (row: DailyTrendRow) => number): number[] {
  const map = new Map<string, number>()
  for (const row of hourlyRows.value) map.set(row.day, (map.get(row.day) || 0) + getter(row))
  return ALL_HOURS.map(h => map.get(h) || 0)
}

const dates = computed<string[]>(() => {
  if (isSingleDay.value && hourlyRows.value.length > 0) return ALL_HOURS
  const f = filterStore.fromDate
  const t = filterStore.toDate
  if (!f || !t) return []
  return generateDateRange(f, t)
})

function buildDailySeries(getter: (day: string) => number): number[] {
  return dates.value.map(d => getter(d) || 0)
}

function makeSeriesData(hourly: (r: DailyTrendRow) => number, daily: (d: string) => number) {
  return computed(() =>
    isSingleDay.value && hourlyRows.value.length > 0 ? aggregateHourly(hourly) : buildDailySeries(daily)
  )
}

const totalCostData = makeSeriesData(r => getCostForRow(r), d => queryStore.precomputed?.dayCostMap?.[d] || 0)
const totalTokenData = makeSeriesData(
  r => r.inputTokens + r.outputTokens + r.cacheRead + r.cacheCreation,
  d => {
    const pre = queryStore.precomputed
    return pre ? (pre.dayInputTokens?.[d] || 0) + (pre.dayOutputTokens?.[d] || 0) + (pre.dayCacheRead?.[d] || 0) + (pre.dayCacheCreation?.[d] || 0) : 0
  }
)
const inputData = makeSeriesData(r => r.inputTokens, d => queryStore.precomputed?.dayInputTokens?.[d] || 0)
const outputData = makeSeriesData(r => r.outputTokens, d => queryStore.precomputed?.dayOutputTokens?.[d] || 0)
const cacheReadData = makeSeriesData(r => r.cacheRead, d => queryStore.precomputed?.dayCacheRead?.[d] || 0)
const cacheCreationData = makeSeriesData(r => r.cacheCreation, d => queryStore.precomputed?.dayCacheCreation?.[d] || 0)

// ===== 持久化有效数据，刷新期间保持图表显示 =====
const display = reactive({
  dates: [] as string[], cost: [] as number[], token: [] as number[],
  input: [] as number[], output: [] as number[], cacheR: [] as number[], cacheW: [] as number[]
})

const allData = computed(() => ({
  dates: dates.value, cost: totalCostData.value, token: totalTokenData.value,
  input: inputData.value, output: outputData.value, cacheR: cacheReadData.value, cacheW: cacheCreationData.value
}))

watch(allData, (v) => {
  if (v.dates.length > 0) {
    Object.assign(display, v)
  } else if (!queryStore.loading && !hourlyLoading.value) {
    display.dates = []; display.cost = []; display.token = []
    display.input = []; display.output = []; display.cacheR = []; display.cacheW = []
  }
})
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
  justify-content: center;
  gap: 6px;
  flex-shrink: 0;
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
