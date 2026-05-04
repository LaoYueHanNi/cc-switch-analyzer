<template>
  <div class="pricing-grid">
    <div
      v-for="item in items"
      :key="item.label"
      class="pricing-row"
    >
      <span class="pricing-dot" :style="{ backgroundColor: item.color }" />
      <span class="pricing-label">{{ item.label }}</span>
      <span class="pricing-tokens">{{ item.tokens }}</span>
      <span class="pricing-cost">{{ item.cost }}</span>
      <span class="pricing-rate">{{ item.rate }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { COLORS } from '@/utils/constants'
import { formatNum, formatCost, formatRate } from '@/utils/format'

interface PricingItem {
  label: string
  tokens: string
  cost: string
  rate: string
  color: string
}

const props = defineProps<{
  inputTokens?: number
  outputTokens?: number
  cacheReadTokens?: number
  cacheCreationTokens?: number
  inputCost?: number
  outputCost?: number
  cacheReadCost?: number
  cacheCreationCost?: number
  inputRate?: string
  outputRate?: string
  cacheReadRate?: string
  cacheCreationRate?: string
  showCost?: boolean
  showTokens?: boolean
}>()

const items = computed<PricingItem[]>(() => [
  {
    label: '输入',
    tokens: formatNum(props.inputTokens ?? 0),
    cost: formatCost(props.inputCost ?? 0),
    rate: props.inputRate ?? '-',
    color: COLORS.PURPLE
  },
  {
    label: '输出',
    tokens: formatNum(props.outputTokens ?? 0),
    cost: formatCost(props.outputCost ?? 0),
    rate: props.outputRate ?? '-',
    color: COLORS.ORANGE
  },
  {
    label: '缓存读取',
    tokens: formatNum(props.cacheReadTokens ?? 0),
    cost: formatCost(props.cacheReadCost ?? 0),
    rate: props.cacheReadRate ?? '-',
    color: COLORS.BLUE
  },
  {
    label: '缓存写入',
    tokens: formatNum(props.cacheCreationTokens ?? 0),
    cost: formatCost(props.cacheCreationCost ?? 0),
    rate: props.cacheCreationRate ?? '-',
    color: COLORS.DARK_ORANGE
  }
])
</script>

<style scoped>
.pricing-grid {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 11px;
}

.pricing-row {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
}

.pricing-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}

.pricing-label {
  flex: 0 0 auto;
  min-width: 28px;
  color: #666;
}

.pricing-tokens {
  flex: 1;
  min-width: 0;
  color: #333;
  text-align: right;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pricing-cost {
  flex: 1;
  min-width: 0;
  color: #e74c3c;
  text-align: right;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pricing-rate {
  flex: 0 0 auto;
  color: #999;
  font-size: 10px;
  white-space: nowrap;
}
</style>
