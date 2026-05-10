<template>
  <div class="model-card">
    <!-- 模型名称 -->
    <div class="card-header">
      <span class="model-name">{{ modelId }}</span>
      <span v-if="hasTimePricing" class="time-badge" :title="timeBadgeTitle">
        <n-icon size="14"><time-outline /></n-icon>
        {{ timeBadgeText }}
      </span>
    </div>

    <!-- 无定价数据提示 -->
    <div v-if="!hasPricing" class="no-pricing">
      <p>暂无定价数据</p>
      <n-button size="tiny" type="primary" @click="$emit('setPricing', modelId)">
        设置定价
      </n-button>
    </div>

    <template v-else>
      <!-- 总费用（可点击对比） -->
      <div class="cost-section">
        <span class="cost-value" @click="$emit('compare', modelId)">{{ formatCost(totalCost) }}</span>
        <span class="cost-label">总费用</span>
      </div>

      <!-- 总 Token + 请求数 -->
      <div class="token-section">
        <span class="token-value">{{ formatNum(totalTokens) }}</span>
        <span class="token-label">总 Token</span>
        <span class="request-count">{{ requestCount }} 次请求</span>
      </div>

      <!-- 统计信息 -->
      <div class="stats-row">
        <span class="stat-item">单次 ¥{{ formatRate(costPerRequest) }}</span>
        <span class="stat-item">命中率 {{ formatPercent(cacheHitRate) }}</span>
      </div>

      <!-- 上下文档位占比 -->
      <div v-if="tierLabel" class="stats-row">
        <span class="stat-item">{{ tierLabel }}</span>
      </div>

      <!-- 费用分解网格 -->
      <PricingGrid
        :input-tokens="modelData?.inputTokens"
        :output-tokens="modelData?.outputTokens"
        :cache-read-tokens="modelData?.cacheRead"
        :cache-creation-tokens="modelData?.cacheCreation"
        :input-cost="costBreakdown[0]"
        :output-cost="costBreakdown[1]"
        :cache-read-cost="costBreakdown[2]"
        :cache-creation-cost="costBreakdown[3]"
        :input-rate="getRateStr('inputCostPerMillion')"
        :output-rate="getRateStr('outputCostPerMillion')"
        :cache-read-rate="getRateStr('cacheReadCostPerMillion')"
        :cache-creation-rate="getRateStr('cacheCreationCostPerMillion')"
      />
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { NButton, NIcon } from 'naive-ui'
import { TimeOutline } from '@vicons/ionicons5'
import { formatNum, formatRate, formatCost, formatPercent, epochToDateStr } from '@/utils/format'
import PricingGrid from '@/components/common/PricingGrid.vue'
import type { ModelBreakdown } from '@/types/database'
import type { PricingData, TimePricingRule, CloudPricingTimeRule } from '@/types/pricing'

const props = defineProps<{
  modelData: ModelBreakdown
  pricing: PricingData | null
  costBreakdown: [number, number, number, number]
  totalCost: number
  hasTimePricing: boolean
  timeRules: TimePricingRule[]
  cloudTimeRules?: CloudPricingTimeRule[]
  contextTierCosts?: Array<{ threshold: number; cost: number }>
}>()

defineEmits<{
  compare: [modelId: string]
  setPricing: [modelId: string]
}>()

const modelId = computed(() => props.modelData.model)
const hasPricing = computed(() => props.pricing !== null)
const totalTokens = computed(() =>
  props.modelData.inputTokens + props.modelData.outputTokens +
  props.modelData.cacheRead + props.modelData.cacheCreation
)

const requestCount = computed(() => props.modelData.requests)
const costPerRequest = computed(() =>
  requestCount.value > 0 ? props.totalCost / requestCount.value : 0
)

const cacheHitRate = computed(() => {
  const input = props.modelData.inputTokens
  const cacheRead = props.modelData.cacheRead
  return (input + cacheRead) > 0 ? cacheRead / (input + cacheRead) : 0
})

const tierLabel = computed(() => {
  const tiers = props.contextTierCosts
  if (!tiers || !tiers.some(t => t.threshold > 0)) return ''
  const total = tiers.reduce((s, t) => s + t.cost, 0)
  if (total <= 0) return ''
  return tiers.map(t => {
    const pct = Math.round(t.cost / total * 100)
    const name = t.threshold === 0 ? '基础' : `≥${Math.round(t.threshold / 1000)}K`
    return `${name} ${pct}%`
  }).join(' ')
})

const allDisplayTimeRules = computed(() => [
  ...props.timeRules.map(r => ({ label: r.label, startTime: r.startTime, endTime: r.endTime })),
  ...(props.cloudTimeRules || []).map(r => ({ label: r.label, startTime: r.startTime, endTime: r.endTime }))
])

const timeBadgeText = computed(() => {
  const labels = allDisplayTimeRules.value.map(r => r.label).filter(Boolean)
  return labels.length > 0 ? labels.join('、') : '时段定价'
})

const timeBadgeTitle = computed(() => {
  if (allDisplayTimeRules.value.length === 0) return '包含时段定价'
  return allDisplayTimeRules.value.map(r => r.label || `${epochToDateStr(r.startTime)} ~ ${epochToDateStr(r.endTime)}`).join('\n')
})

type RateField = 'inputCostPerMillion' | 'outputCostPerMillion' | 'cacheReadCostPerMillion' | 'cacheCreationCostPerMillion'

function getRateStr(field: RateField): string {
  if (props.hasTimePricing && props.pricing?.timeRules?.length) {
    const rules = props.pricing.timeRules
    if (rules.length === 1) return formatRate(rules[0][field]) + '/M'
    return rules.map(r => (r.label ? r.label + ':' : '') + formatRate(r[field]) + '/M').join(' ')
  }
  return formatRate((props.pricing?.[field] as number) || 0) + '/M'
}

</script>

<style scoped>
.model-card {
  position: relative;
  background: var(--bg-card);
  border-radius: 6px;
  border: 1px solid var(--border-main);
  padding: 10px;
  min-width: 0;
  overflow: hidden;
  transition: box-shadow 0.2s;
}

.model-card:hover {
  box-shadow: 0 2px 6px rgba(0,0,0,0.06);
}

.card-header {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-bottom: 4px;
  min-width: 0;
}

.model-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.time-badge {
  font-size: 10px;
  color: var(--color-orange);
  display: flex;
  align-items: center;
  gap: 2px;
}

.no-pricing {
  text-align: center;
  padding: 12px 0;
  color: var(--text-muted);
  font-size: 12px;
}

.cost-section {
  display: flex;
  align-items: baseline;
  gap: 4px;
  margin-bottom: 2px;
}

.cost-value {
  font-size: 17px;
  font-weight: 700;
  color: var(--color-cost);
  cursor: pointer;
  text-decoration: underline;
  text-decoration-style: dotted;
  text-underline-offset: 3px;
  transition: opacity 0.2s;
}

.cost-value:hover {
  opacity: 0.7;
}

.cost-label {
  font-size: 10px;
  color: var(--text-muted);
}

.token-section {
  display: flex;
  align-items: baseline;
  gap: 4px;
  margin-bottom: 4px;
}

.token-value {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-green);
}

.token-label {
  font-size: 10px;
  color: var(--text-muted);
}

.stats-row {
  display: flex;
  flex-wrap: nowrap;
  overflow: hidden;
  gap: 4px 8px;
  margin-bottom: 4px;
  font-size: 10px;
}

.stat-item {
  color: var(--text-tertiary);
  white-space: nowrap;
}

.request-count {
  font-size: 10px;
  color: var(--text-faint);
  margin-left: auto;
  white-space: nowrap;
}
</style>
