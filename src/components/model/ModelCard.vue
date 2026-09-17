<template>
  <div
    class="model-card"
    :class="{ open }"
    @click="onCardClick"
  >
    <!-- 模型名称 -->
    <div class="card-header">
      <span class="model-name">{{ modelId }}</span>
      <span
        v-if="!hasPricing"
        class="time-badge unpriced-badge"
        title="该模型未配置定价，费用按 0 计算；点击设置定价"
        @click.stop="$emit('setPricing', modelId)"
      >缺少定价</span>
      <span v-if="timeBadgeText" class="time-badge" :title="timeBadgeTitle">
        <n-icon size="14"><time-outline /></n-icon>
        {{ timeBadgeText }}
      </span>
    </div>

    <!-- 总费用（可点击对比） -->
    <div class="cost-section">
      <span
        class="cost-value"
        :class="{ 'cost-value--muted': !hasPricing }"
        @click.stop="onCostClick"
      >{{ formatCost(totalCost) }}</span>
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

    <!-- 计费明细入口 -->
    <div v-if="showBreakdownBtn" class="breakdown-btn" @click.stop="showBreakdownDialog = true">计费明细</div>
    <PricingBreakdownDialog
      v-model:show="showBreakdownDialog"
      :model-name="modelId"
      :compare-buckets="compareBuckets"
      :pricing="pricing"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { NIcon } from 'naive-ui'
import { TimeOutline } from '@vicons/ionicons5'
import { formatNum, formatRate, formatCost, formatPercent, epochToDateStr } from '@/utils/format'
import PricingGrid from '@/components/common/PricingGrid.vue'
import PricingBreakdownDialog from './PricingBreakdownDialog.vue'
import type { ModelBreakdown } from '@/types/database'
import type { PricingData, TimePricingRule, CloudPricingTimeRule } from '@/types/pricing'
import type { CompareBucket } from '@/types/common'

const props = defineProps<{
  modelData: ModelBreakdown
  pricing: PricingData | null
  costBreakdown: [number, number, number, number]
  totalCost: number
  hasTimePricing: boolean
  timeRules: TimePricingRule[]
  cloudTimeRules?: CloudPricingTimeRule[]
  contextTierCosts?: Array<{ threshold: number; cost: number; tokens: number }>
  compareBuckets?: CompareBucket[]
  open?: boolean
}>()

const emit = defineEmits<{
  compare: [modelId: string]
  setPricing: [modelId: string]
  pin: [modelId: string]
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
  const cacheCreation = props.modelData.cacheCreation
  return (input + cacheRead + cacheCreation) > 0 ? cacheRead / (input + cacheRead + cacheCreation) : 0
})

const allDisplayTimeRules = computed(() => [
  ...props.timeRules.map(r => ({
    label: r.label, startTime: r.startTime, endTime: r.endTime,
    inputCostPerMillion: r.inputCostPerMillion,
    outputCostPerMillion: r.outputCostPerMillion,
    cacheReadCostPerMillion: r.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: r.cacheCreationCostPerMillion
  })),
  ...(props.cloudTimeRules || []).map(r => ({
    label: r.label, startTime: r.startTime, endTime: r.endTime,
    inputCostPerMillion: r.inputCostPerMillion,
    outputCostPerMillion: r.outputCostPerMillion,
    cacheReadCostPerMillion: r.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: r.cacheCreationCostPerMillion
  }))
])

// 仅当前生效的时间规则
const activeTimeRules = computed(() => {
  const now = Math.floor(Date.now() / 1000)
  return allDisplayTimeRules.value.filter(r => now >= r.startTime && now <= r.endTime)
})

const timeBadgeText = computed(() => {
  if (activeTimeRules.value.length === 0) return ''
  const labels = activeTimeRules.value.map(r => r.label).filter(Boolean)
  return labels.length > 0 ? labels.join('、') : '时段定价'
})

const timeBadgeTitle = computed(() => {
  if (activeTimeRules.value.length === 0) return ''
  return activeTimeRules.value.map(r => r.label || `${epochToDateStr(r.startTime)} ~ ${epochToDateStr(r.endTime)}`).join('\n')
})

type RateField = 'inputCostPerMillion' | 'outputCostPerMillion' | 'cacheReadCostPerMillion' | 'cacheCreationCostPerMillion'

function getEffectiveRate(field: RateField): number {
  const bd = props.costBreakdown
  const mb = props.modelData
  const M = 1_000_000
  const fieldMap: Record<RateField, [number, number]> = {
    inputCostPerMillion: [bd[0], mb.inputTokens],
    outputCostPerMillion: [bd[1], mb.outputTokens],
    cacheReadCostPerMillion: [bd[2], mb.cacheRead],
    cacheCreationCostPerMillion: [bd[3], mb.cacheCreation],
  }
  const [cost, tokens] = fieldMap[field]
  return tokens > 0 ? cost * M / tokens : 0
}

function getRateStr(field: RateField): string {
  if (activeTimeRules.value.length > 0) {
    const rules = activeTimeRules.value
    if (rules.length === 1) return formatRate(rules[0][field]) + '/M'
    return rules.map(r => (r.label ? r.label + ':' : '') + formatRate(r[field]) + '/M').join(' ')
  }
  const rate = getEffectiveRate(field)
  if (rate > 0) return formatRate(rate) + '/M'
  return formatRate((props.pricing?.[field] as number) || 0) + '/M'
}

const showBreakdownDialog = ref(false)
function onCardClick(): void {
  emit('pin', modelId.value)
}
// 无定价时费用恒为 0，对比没有意义
function onCostClick(): void {
  if (!hasPricing.value) return
  emit('compare', modelId.value)
}
const showBreakdownBtn = computed(() => {
  if (!props.compareBuckets?.length || !props.pricing) return false
  // 实际命中了多个档位（至少两个 threshold 不同）
  const usedTiers = new Set(props.compareBuckets.map(b => b.threshold))
  if (usedTiers.size > 1) return true
  // 实际命中了时间定价（有 bucket 的 epoch 落在时间规则范围内）
  const allRules = [...props.timeRules, ...(props.cloudTimeRules || [])]
  if (allRules.length > 0 && props.compareBuckets.some(b => allRules.some(r => b.representativeEpoch >= r.startTime && b.representativeEpoch <= r.endTime))) return true
  return false
})

</script>

<style scoped>
.model-card {
  position: relative;
  background: var(--bg-card);
  border-radius: var(--card-radius);
  border: 0;
  box-shadow: var(--shadow-card);
  padding: 10px;
  min-width: 0;
  overflow: hidden;
  cursor: pointer;
  transition: box-shadow var(--transition-speed), background var(--transition-speed);
}

.model-card:hover,
.model-card.open {
  box-shadow: var(--shadow-focus);
  background: var(--bg-hover);
}

.model-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cost-value {
  font-size: var(--font-size-cost);
  font-weight: 700;
  color: var(--color-cost);
  cursor: pointer;
  letter-spacing: -0.03em;
  text-decoration: underline;
  text-decoration-style: dotted;
  text-underline-offset: 3px;
  transition: opacity var(--transition-speed);
}

.token-value {
  font-size: 14px;
  font-weight: 600;
  color: var(--color-green);
}

.stats-row {
  display: flex;
  flex-wrap: nowrap;
  overflow: hidden;
  gap: 4px 8px;
  margin-bottom: 4px;
  font-size: 11px;
}

.model-card :deep(.pricing-grid) {
  opacity: 0.16;
  filter: saturate(0.45);
  transition: opacity var(--transition-speed), filter var(--transition-speed);
}

.model-card:hover :deep(.pricing-grid),
.model-card.open :deep(.pricing-grid) {
  opacity: 1;
  filter: none;
}

.card-header {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-bottom: 4px;
  min-width: 0;
}

.time-badge {
  font-size: 10px;
  color: var(--color-orange);
  display: flex;
  align-items: center;
  gap: 2px;
}

/* 缺少定价标识：复用时段定价徽标的视觉，点击进入原地定价 */
.unpriced-badge {
  color: var(--color-amber);
  cursor: pointer;
  white-space: nowrap;
}

.unpriced-badge:hover {
  opacity: 0.75;
}

.cost-section {
  display: flex;
  align-items: baseline;
  gap: 4px;
  margin-bottom: 2px;
}

.cost-value:hover {
  opacity: 0.7;
}

/* 无定价：费用恒为 0，降级显示且不可点击对比 */
.cost-value--muted,
.cost-value--muted:hover {
  color: var(--text-muted);
  text-decoration: none;
  cursor: default;
  opacity: 1;
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

.token-label {
  font-size: 10px;
  color: var(--text-muted);
}

.stat-item {
  color: var(--text-secondary);
  font-weight: 500;
  white-space: nowrap;
}

.request-count {
  font-size: 10px;
  color: var(--text-faint);
  margin-left: auto;
  white-space: nowrap;
}

.breakdown-btn {
  margin-top: 4px;
  font-size: 10px;
  color: var(--color-blue);
  cursor: pointer;
}
.breakdown-btn:hover { opacity: 0.7; }
</style>
