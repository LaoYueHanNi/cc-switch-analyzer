<template>
  <div class="filter-bar">
    <div class="filter-row">
      <div class="filter-group">
        <span class="filter-label">数据源</span>
        <CompactSelect
          :model-value="filterStore.providerId"
          :options="providerSelectOptions"
          clearable
          placeholder="全部"
          @update:model-value="filterStore.providerId = $event"
        />
      </div>
      <div class="filter-group">
        <span class="filter-label">模型</span>
        <CompactSelect
          :model-value="filterStore.modelId"
          :options="modelSelectOptions"
          clearable
          placeholder="全部"
          @update:model-value="filterStore.modelId = $event"
        />
      </div>
      <div class="filter-group">
        <span class="filter-label">日期</span>
        <CompactDateRange v-model:value="dateRange" />
      </div>
      <div class="quick-dates">
        <button
          v-for="btn in quickDateButtons"
          :key="btn.days"
          class="quick-btn"
          :class="{ active: activeQuickDays === btn.days }"
          :disabled="!dbStore.hasDatabase"
          @click="onQuickDate(btn.days)"
        >
          {{ btn.label }}
        </button>
      </div>
      <div class="filter-actions">
        <button class="action-btn" :disabled="!dbStore.hasDatabase" @click="onAll">所有</button>
        <button class="action-btn primary" :disabled="!dbStore.hasDatabase" @click="onQuery">查询</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import CompactSelect from '@/components/common/CompactSelect.vue'
import CompactDateRange from '@/components/common/CompactDateRange.vue'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'
import { useFilter } from '@/composables/useFilter'

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const queryStore = useQueryStore()
const { quickDateQuery } = useFilter()

const activeQuickDays = computed({
  get: () => filterStore.activeQuickDays,
  set: (v) => { filterStore.activeQuickDays = v }
})
const dateRange = ref<[number, number] | null>(null)
let skipActiveReset = false

watch(dateRange, (val) => {
  if (skipActiveReset) {
    skipActiveReset = false
    return
  }
  if (val) {
    filterStore.fromDate = new Date(val[0])
    filterStore.toDate = new Date(val[1])
    activeQuickDays.value = null
  } else {
    filterStore.fromDate = null
    filterStore.toDate = null
    activeQuickDays.value = null
  }
})

watch([() => filterStore.fromDate, () => filterStore.toDate], ([f, t]) => {
  if (f && t) {
    const ts = [f.getTime(), t.getTime()] as [number, number]
    if (!dateRange.value || dateRange.value[0] !== ts[0] || dateRange.value[1] !== ts[1]) {
      skipActiveReset = true
      dateRange.value = ts
    }
  }
}, { immediate: true })

const quickDateButtons = [
  { label: '1天', days: 1 },
  { label: '7天', days: 7 },
  { label: '30天', days: 30 },
  { label: '60天', days: 60 },
  { label: '180天', days: 180 }
]

const providerSelectOptions = computed(() => [...filterStore.providerOptions])

const modelSelectOptions = computed(() => [...filterStore.modelOptions])

async function onQuery(): Promise<void> {
  await queryStore.executeQuery(filterStore.filterParams, true)
}

async function onAll(): Promise<void> {
  filterStore.reset()
  if (filterStore.dateRangeMin != null && filterStore.dateRangeMax != null) {
    filterStore.fromDate = new Date(filterStore.dateRangeMin * 1000)
    filterStore.toDate = new Date(filterStore.dateRangeMax * 1000)
  }
  await queryStore.executeQuery(filterStore.filterParams, true)
}

async function onQuickDate(days: number): Promise<void> {
  activeQuickDays.value = days
  await quickDateQuery(days)
}
</script>

<style scoped>
.filter-bar {
  padding: 4px 12px 6px;
  border-top: 1px solid var(--border-light);
}

.filter-row {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.filter-group {
  display: flex;
  align-items: center;
  gap: 3px;
}

.filter-label {
  font-size: 11px;
  color: var(--text-tertiary);
  white-space: nowrap;
}

.quick-dates {
  display: flex;
  gap: 2px;
}

.quick-btn {
  font-size: 10px;
  padding: 1px 6px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  line-height: 1.4;
}

.quick-btn:hover {
  border-color: var(--color-blue);
  color: var(--color-blue);
}

.quick-btn.active {
  background: var(--color-blue);
  border-color: var(--color-blue);
  color: #fff;
}

.quick-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.filter-actions {
  display: flex;
  gap: 4px;
  margin-left: 4px;
}

.action-btn {
  font-size: 10px;
  padding: 1px 8px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  line-height: 1.4;
}

.action-btn.primary {
  background: var(--color-blue);
  border-color: var(--color-blue);
  color: #fff;
}

.action-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
</style>
