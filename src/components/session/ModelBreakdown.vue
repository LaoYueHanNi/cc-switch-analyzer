<template>
  <div class="model-breakdown">
    <div
      v-for="item in items"
      :key="item.model"
      class="model-block"
    >
      <div class="model-name" :title="item.model">{{ truncate(item.model, 20) }}</div>
      <div class="model-rows">
        <div class="model-row">
          <span class="dot" style="background:var(--color-purple)" />
          <span class="val">{{ formatNum(item.inputTokens) }}</span>
          <span class="cost">{{ formatCost(item.inputCost || 0) }}</span>
        </div>
        <div class="model-row">
          <span class="dot" style="background:var(--color-orange)" />
          <span class="val">{{ formatNum(item.outputTokens) }}</span>
          <span class="cost">{{ formatCost(item.outputCost || 0) }}</span>
        </div>
        <div class="model-row">
          <span class="dot" style="background:var(--color-blue)" />
          <span class="val">{{ formatNum(item.cacheRead) }}</span>
          <span class="cost">{{ formatCost(item.cacheReadCost || 0) }}</span>
        </div>
        <div class="model-row">
          <span class="dot" style="background:var(--color-dark-orange)" />
          <span class="val">{{ formatNum(item.cacheCreation) }}</span>
          <span class="cost">{{ formatCost(item.cacheCreationCost || 0) }}</span>
        </div>
      </div>
      <div class="model-total">{{ formatCost(item.totalCost || 0) }}</div>
        <div v-if="cacheHitRate(item) !== null" class="model-cache-rate">缓存: {{ cacheHitRate(item) }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { formatNum, formatCost, formatPercent } from '@/utils/format'

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
  }>
}>()

function truncate(s: string, max: number): string {
  return s.length > max ? s.slice(0, max) + '...' : s
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
  gap: 8px;
  flex-wrap: wrap;
}

.model-block {
  min-width: 140px;
  padding: 8px;
  background: var(--bg-card-alt);
  border-radius: 4px;
  border: 1px solid var(--border-light);
}

.model-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 4px;
  cursor: default;
}

.model-rows {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.model-row {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}

.val {
  color: var(--text-primary);
  min-width: 40px;
  text-align: right;
}

.cost {
  color: var(--color-cost);
  min-width: 55px;
  text-align: right;
}

.model-total {
  margin-top: 4px;
  font-size: 12px;
  font-weight: 600;
  color: var(--color-cost);
  text-align: right;
  border-top: 1px solid var(--border-faint);
  padding-top: 4px;
}

.model-cache-rate {
  font-size: 10px;
  color: var(--text-muted);
  text-align: right;
  margin-top: 1px;
}
</style>
