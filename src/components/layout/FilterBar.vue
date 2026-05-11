<template>
  <div class="filter-bar">
    <!-- 第一行：日期 + 快捷日期 + 查询/重置 -->
    <div class="filter-row">
      <div class="filter-group">
        <span class="filter-label">日期</span>
        <n-date-picker
          v-model:value="dateRange"
          type="daterange"
          size="tiny"
          clearable
          style="width: 200px"
        />
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
        <button class="action-btn primary" :disabled="!dbStore.hasDatabase" @click="onQuery">查询</button>
        <button class="action-btn" :disabled="!dbStore.hasDatabase" @click="onReset">重置</button>
      </div>
    </div>

    <!-- 第二行：供应商 + 模型 -->
    <div class="filter-row">
      <div class="filter-group">
        <span class="filter-label">供应商</span>
        <n-select
          v-model:value="filterStore.providerId"
          :options="providerSelectOptions"
          size="tiny"
          clearable
          style="width: 150px"
          placeholder="全部"
          teleport-disabled
        />
      </div>
      <div class="filter-group">
        <span class="filter-label">模型</span>
        <n-select
          v-model:value="filterStore.modelId"
          :options="modelSelectOptions"
          size="tiny"
          clearable
          style="width: 150px"
          placeholder="全部"
          teleport-disabled
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { NDatePicker, NSelect } from 'naive-ui'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'
import { useFilter } from '@/composables/useFilter'

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const queryStore = useQueryStore()
const { quickDateQuery } = useFilter()

const activeQuickDays = ref<number | null>(30)
const dateRange = ref<[number, number] | null>(null)

watch(dateRange, (val) => {
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

const providerSelectOptions = computed(() => [
  { label: '全部供应商', value: '' },
  ...filterStore.providerOptions
])

const modelSelectOptions = computed(() => [
  { label: '全部模型', value: '' },
  ...filterStore.modelOptions
])

async function onQuery(): Promise<void> {
  await queryStore.executeQuery(filterStore.filterParams, true)
}

function onReset(): void {
  filterStore.reset()
  dateRange.value = null
  activeQuickDays.value = null
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
  margin-bottom: 4px;
}

.filter-row:last-child {
  margin-bottom: 0;
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
