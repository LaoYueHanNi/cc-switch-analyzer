<template>
  <div class="pricing-grid">
    <template v-for="item in items" :key="item.label">
      <span class="pricing-dot" :style="{ backgroundColor: item.color }" />
      <span class="pricing-label">{{ item.label }}</span>
      <span class="pricing-tokens">{{ item.tokens }}</span>
      <span class="pricing-cost">{{ item.cost }}</span>
      <span class="pricing-rate">{{ item.rate }}</span>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
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
}>()

const items = computed<PricingItem[]>(() => [
  {
    label: '输入',
    tokens: formatNum(props.inputTokens ?? 0),
    cost: formatCost(props.inputCost ?? 0),
    rate: props.inputRate ?? '-',
    color: 'var(--color-purple)'
  },
  {
    label: '输出',
    tokens: formatNum(props.outputTokens ?? 0),
    cost: formatCost(props.outputCost ?? 0),
    rate: props.outputRate ?? '-',
    color: 'var(--color-orange)'
  },
  {
    label: '缓存读取',
    tokens: formatNum(props.cacheReadTokens ?? 0),
    cost: formatCost(props.cacheReadCost ?? 0),
    rate: props.cacheReadRate ?? '-',
    color: 'var(--color-blue)'
  },
  {
    label: '缓存写入',
    tokens: formatNum(props.cacheCreationTokens ?? 0),
    cost: formatCost(props.cacheCreationCost ?? 0),
    rate: props.cacheCreationRate ?? '-',
    color: 'var(--color-dark-orange)'
  }
])
</script>

<style scoped>
.pricing-grid {
  display: grid;
  grid-template-columns: 6px auto 1fr auto auto;
  gap: 2px 4px;
  font-size: 11px;
  align-items: center;
}

.pricing-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.pricing-label {
  color: var(--text-tertiary);
  white-space: nowrap;
}

.pricing-tokens {
  color: var(--text-primary);
  text-align: right;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pricing-cost {
  color: var(--color-cost);
  text-align: right;
  white-space: nowrap;
}

.pricing-rate {
  color: var(--text-muted);
  font-size: 10px;
  text-align: right;
  white-space: nowrap;
}
</style>
