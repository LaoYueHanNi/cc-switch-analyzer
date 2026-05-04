<template>
  <div class="model-breakdown">
    <div
      v-for="item in items"
      :key="item.model"
      class="model-block"
    >
      <div class="mb-header">
        <span class="mb-name" :title="item.model">{{ truncate(item.model, 20) }}</span>
        <span class="mb-tokens">{{ formatNum(item.totalTokens) }}</span>
        <span class="mb-total">{{ formatCost(item.totalCost || 0) }}</span>
      </div>
      <div class="mb-grid">
        <template v-for="r in rows(item)" :key="r.label">
          <span class="dot" :style="{ backgroundColor: r.color }" />
          <span class="mb-label">{{ r.label }}</span>
          <span class="mb-val">{{ r.tokens }}</span>
          <span class="mb-cost">{{ r.cost }}</span>
        </template>
      </div>
      <div v-if="cacheHitRate(item) !== null" class="mb-cache">缓存 {{ cacheHitRate(item) }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { formatNum, formatCost, formatPercent } from '@/utils/format'
import { COLORS } from '@/utils/constants'

defineProps<{
  items: Array<{
    model: string
    inputTokens: number
    outputTokens: number
    cacheRead: number
    cacheCreation: number
    inputCost?: number
    outputCost?: number
    cacheReadCost?: number
    cacheCreationCost?: number
    totalCost?: number
    totalTokens?: number
  }>
}>()

function truncate(s: string, max: number): string {
  return s.length > max ? s.slice(0, max) + '...' : s
}

function rows(item: {
  inputTokens: number; outputTokens: number; cacheRead: number; cacheCreation: number
  inputCost?: number; outputCost?: number; cacheReadCost?: number; cacheCreationCost?: number
}) {
  return [
    { label: '输入', tokens: formatNum(item.inputTokens), cost: formatCost(item.inputCost || 0), color: COLORS.PURPLE },
    { label: '输出', tokens: formatNum(item.outputTokens), cost: formatCost(item.outputCost || 0), color: COLORS.ORANGE },
    { label: '缓存读', tokens: formatNum(item.cacheRead), cost: formatCost(item.cacheReadCost || 0), color: COLORS.BLUE },
    { label: '缓存写', tokens: formatNum(item.cacheCreation), cost: formatCost(item.cacheCreationCost || 0), color: COLORS.DARK_ORANGE }
  ]
}

function cacheHitRate(item: { inputTokens: number; cacheRead: number }): string | null {
  const total = item.inputTokens + item.cacheRead
  if (total <= 0) return null
  return formatPercent(item.cacheRead / total)
}
</script>

<style scoped>
.model-breakdown {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.model-block {
  min-width: 160px;
  padding: 6px 8px;
  background: var(--bg-card-alt);
  border-radius: 4px;
  border: 1px solid var(--border-light);
}

.mb-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  margin-bottom: 4px;
}

.mb-name {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0;
}

.mb-tokens {
  font-size: 10px;
  color: var(--color-green);
  flex-shrink: 0;
}

.mb-total {
  font-size: 11px;
  font-weight: 600;
  color: var(--color-cost);
  flex-shrink: 0;
}

.mb-grid {
  display: grid;
  grid-template-columns: 5px auto 1fr auto;
  gap: 1px 4px;
  font-size: 10px;
  align-items: center;
}

.dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
}

.mb-label {
  color: var(--text-muted);
  white-space: nowrap;
}

.mb-val {
  color: var(--text-primary);
  text-align: right;
  white-space: nowrap;
}

.mb-cost {
  color: var(--color-cost);
  text-align: right;
  white-space: nowrap;
}

.mb-cache {
  font-size: 9px;
  color: var(--text-muted);
  text-align: right;
  margin-top: 2px;
}
</style>
