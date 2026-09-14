<template>
  <div v-if="queryStore.unpricedModels.length > 0" class="unpriced-warning">
    <span class="warning-icon">!</span>
    <span>{{ queryStore.unpricedModels.length }} 个模型缺少定价配置：{{ queryStore.unpricedModels.join('、') }}</span>
  </div>
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
.unpriced-warning {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 12px;
  margin: 0 12px;
  background: var(--color-amber-bg);
  border: 1px solid var(--color-amber);
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-muted);
}

.warning-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--color-amber);
  color: #fff;
  font-size: 11px;
  font-weight: 700;
  flex-shrink: 0;
}

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
