<template>
  <div class="summary-bar">
    <div
      v-for="item in items"
      :key="item.key"
      class="summary-card"
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

  return SUMMARY_ITEMS.map(item => {
    let rawValue: number | string = '-'

    switch (item.key) {
      case 'totalRequests':
        rawValue = s?.totalRequests ?? '-'
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
      case 'totalCacheRead':
        rawValue = s?.totalCacheRead ?? '-'
        break
      case 'totalCacheCreation':
        rawValue = s?.totalCacheCreation ?? '-'
        break
      case 'totalTokens':
        if (s) {
          rawValue = (s.totalInput || 0) + (s.totalOutput || 0) + (s.totalCacheRead || 0) + (s.totalCacheCreation || 0)
        }
        break
      case 'cacheHitRate':
        if (s) {
          const total = (s.totalInput || 0) + (s.totalCacheRead || 0) + (s.totalCacheCreation || 0)
          rawValue = total > 0 ? (s.totalCacheRead || 0) / total * 100 : 0
        }
        break
    }

    let displayValue: string
    if (typeof rawValue === 'number') {
      if (item.key === 'totalCost') {
        displayValue = formatCost(rawValue)
      } else if (item.key === 'cacheHitRate') {
        displayValue = rawValue.toFixed(1) + '%'
      } else {
        displayValue = formatNum(rawValue)
      }
    } else {
      displayValue = '-'
    }

    return { ...item, displayValue }
  })
})
</script>

<style scoped>
.summary-bar {
  display: flex;
  flex-wrap: wrap;
  gap: 0;
  padding: 10px 8px 4px 16px;
  background: transparent;
}

.summary-card {
  flex: none;
  min-width: 0;
  padding: 2px 18px 2px 0;
  background: none;
  border: 0;
  border-radius: 0;
}

.summary-card + .summary-card {
  padding-left: 18px;
  box-shadow: -0.5px 0 0 var(--border-light);
}

.summary-label {
  font-size: 10px;
  color: var(--text-muted);
  white-space: nowrap;
}

.summary-value {
  font-size: 15px;
  font-weight: 600;
  margin-top: 1px;
  letter-spacing: -0.02em;
}
</style>
