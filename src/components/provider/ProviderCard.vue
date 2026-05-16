<template>
  <div class="provider-card">
    <div class="provider-name">{{ name }}</div>
    <div class="cost-section">
      <span class="cost-value">{{ formatCost(totalCost) }}</span>
      <span class="cost-label">总费用</span>
    </div>
    <div class="request-count">{{ requestCount }} 次请求</div>
  </div>
</template>

<script setup lang="ts">
import { formatCost } from '@/utils/format'

defineProps<{
  name: string
  totalCost: number
  requestCount: number
}>()
</script>

<style scoped>
.provider-card {
  background: var(--bg-card);
  border-radius: 6px;
  border: 1px solid var(--border-main);
  padding: 10px;
  min-width: 0;
  overflow: hidden;
  transition: box-shadow var(--transition-speed);
}

.provider-card:hover {
  box-shadow: var(--shadow-card);
}

.provider-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cost-section {
  display: flex;
  align-items: baseline;
  gap: 4px;
  margin-bottom: 4px;
}

.cost-value {
  font-size: var(--font-size-cost);
  font-weight: 700;
  color: var(--color-cost);
}

.cost-label {
  font-size: 10px;
  color: var(--text-muted);
}

.request-count {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-bottom: 2px;
}
</style>
