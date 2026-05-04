<template>
  <div class="summary-bar">
    <div
      v-for="item in items"
      :key="item.key"
      class="summary-card"
      :style="{ borderLeftColor: item.color }"
    >
      <div class="summary-label">{{ item.label }}</div>
      <div class="summary-value" :style="{ color: item.color }">
        {{ item.displayValue }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useQueryStore } from '@/stores/query'
import { SUMMARY_ITEMS } from '@/utils/constants'
import { formatNum, formatCost } from '@/utils/format'

const queryStore = useQueryStore()

const items = computed(() => {
  const s = queryStore.summary
  const pre = queryStore.precomputed

  return SUMMARY_ITEMS.map(item => {
    let rawValue: number | string = '-'

    switch (item.key) {
      case 'totalRequests':
        rawValue = s?.totalRequests ?? '-'
        break
      case 'successCount':
        rawValue = s?.successCount ?? '-'
        break
      case 'totalCost':
        rawValue = queryStore.totalCost ?? '-'
        break
      case 'totalInput':
        rawValue = s?.totalInput ?? '-'
        break
      case 'totalOutput':
        rawValue = s?.totalOutput ?? '-'
        break
      case 'avgLatency':
        rawValue = s?.avgLatency ?? '-'
        break
      case 'totalCacheRead':
        rawValue = s?.totalCacheRead ?? '-'
        break
      case 'totalCacheCreation':
        rawValue = s?.totalCacheCreation ?? '-'
        break
    }

    let displayValue: string
    if (typeof rawValue === 'number') {
      if (item.key === 'totalCost') {
        displayValue = formatCost(rawValue)
      } else if (item.key === 'avgLatency') {
        displayValue = Math.round(rawValue) + ' ms'
      } else {
        displayValue = formatNum(rawValue)
      }
    } else {
      displayValue = '-'
    }

    return {
      ...item,
      rawValue,
      displayValue
    }
  })
})
</script>

<style scoped>
.summary-bar {
  display: flex;
  gap: 8px;
  padding: 6px 12px 8px;
  overflow-x: auto;
}

.summary-card {
  flex-shrink: 0;
  min-width: 110px;
  padding: 6px 10px;
  background: var(--bg-card);
  border-radius: 4px;
  border: 1px solid var(--border-main);
  border-left-width: 3px;
}

.summary-label {
  font-size: 11px;
  color: var(--text-muted);
  white-space: nowrap;
}

.summary-value {
  font-size: 14px;
  font-weight: 600;
  margin-top: 2px;
}
</style>
