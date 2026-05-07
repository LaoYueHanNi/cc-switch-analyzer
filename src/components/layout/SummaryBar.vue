<template>
  <div class="summary-bar">
    <div
      v-for="item in items"
      :key="item.key"
      class="summary-card"
      :style="{ borderLeftColor: item.color }"
    >
      <div class="summary-label">{{ item.label }}</div>
      <div class="summary-row">
        <div class="summary-value" :style="{ color: item.color }">
          {{ item.displayValue }}
        </div>
        <div v-if="item.tiers?.length" class="summary-tiers">
          <span v-for="t in item.tiers" :key="t.name" class="tier-item">{{ t.name }} {{ t.pct }}%</span>
        </div>
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
    }

    let displayValue: string
    if (typeof rawValue === 'number') {
      if (item.key === 'totalCost') {
        displayValue = formatCost(rawValue)
      } else {
        displayValue = formatNum(rawValue)
      }
    } else {
      displayValue = '-'
    }

    let tiers: Array<{ name: string; pct: number }> | undefined
    if (item.key === 'totalCost' && pre?.contextTierCosts?.length >= 2) {
      const total = pre.contextTierCosts.reduce((sum, t) => sum + t.cost, 0)
      if (total > 0) {
        tiers = pre.contextTierCosts.map(t => ({
          name: t.threshold === 0 ? '基础' : `≥${Math.round(t.threshold / 1000)}K`,
          pct: Math.round(t.cost / total * 100)
        }))
      }
    }

    return { ...item, rawValue, displayValue, tiers }
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

.summary-row {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  margin-top: 2px;
}

.summary-value {
  font-size: 14px;
  font-weight: 600;
  white-space: nowrap;
}

.summary-tiers {
  display: flex;
  flex-direction: column;
  gap: 1px;
  font-size: 9px;
  color: var(--text-muted);
  line-height: 1.2;
  margin-top: -1px;
}

.tier-item {
  white-space: nowrap;
}
</style>
