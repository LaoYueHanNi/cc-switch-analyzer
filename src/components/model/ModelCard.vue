<template>
  <div class="model-card">
    <!-- 模型名称 -->
    <div class="card-header">
      <span class="model-name">{{ displayName }}</span>
      <span v-if="hasTimePricing" class="time-badge" title="包含时段定价">
        <n-icon size="14"><time-outline /></n-icon>
        时段定价
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

      <!-- 总 Token -->
      <div class="token-section">
        <span class="token-value">{{ formatNum(totalTokens) }}</span>
        <span class="token-label">总 Token</span>
      </div>

      <!-- 统计信息 -->
      <div class="stats-row">
        <div class="stat-item">
          <span class="stat-label">单次请求费用</span>
          <span class="stat-num">¥{{ formatRate(costPerRequest) }}</span>
        </div>
        <div class="stat-item">
          <span class="stat-label">缓存命中率</span>
          <span class="stat-num">{{ formatPercent(cacheHitRate) }}</span>
        </div>
        <div class="stat-item clickable" @click="$emit('cacheWindows', modelId, $event)" title="点击查看缓存窗口详情">
          <span class="stat-label">平均缓存时长</span>
          <span class="stat-num clickable-num">{{ avgCacheDuration }}</span>
        </div>
      </div>

      <!-- 请求数 -->
      <div class="request-count">{{ requestCount }} 次请求</div>

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
import { formatNum, formatRate, formatCost, formatPercent, formatDuration } from '@/utils/format'
import PricingGrid from '@/components/common/PricingGrid.vue'
import type { ModelBreakdown } from '@/types/database'
import type { PricingData } from '@/types/pricing'

const props = defineProps<{
  modelData: ModelBreakdown
  pricing: PricingData | null
  costBreakdown: [number, number, number, number]  // [input, output, cacheRead, cacheCreation]
  totalCost: number
  cacheDurationSec: number
  hasTimePricing: boolean
}>()

defineEmits<{
  compare: [modelId: string]
  cacheWindows: [modelId: string, event: MouseEvent]
  setPricing: [modelId: string]
}>()

const modelId = computed(() => props.modelData.model)
const displayName = computed(() => props.pricing?.displayName || props.modelData.model)
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

const avgCacheDuration = computed(() => formatDuration(props.cacheDurationSec))

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
  background: #fff;
  border-radius: 6px;
  border: 1px solid #e8e8e8;
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
  color: #333;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.time-badge {
  font-size: 10px;
  color: #f39c12;
  display: flex;
  align-items: center;
  gap: 2px;
}

.no-pricing {
  text-align: center;
  padding: 12px 0;
  color: #999;
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
  color: #e74c3c;
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
  color: #999;
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
  color: #16a085;
}

.token-label {
  font-size: 10px;
  color: #999;
}

.stats-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 10px;
  margin-bottom: 4px;
  font-size: 11px;
}

.stat-item {
  display: flex;
  flex-direction: column;
}

.stat-item.clickable {
  cursor: pointer;
}

.clickable-num {
  text-decoration: underline;
  text-decoration-style: dotted;
  text-underline-offset: 3px;
}

.stat-item.clickable:hover {
  opacity: 0.7;
}

.stat-label {
  color: #999;
  font-size: 10px;
}

.stat-num {
  color: #333;
  font-weight: 500;
  font-size: 11px;
}

.request-count {
  font-size: 11px;
  color: #bbb;
  margin-bottom: 6px;
}
</style>
