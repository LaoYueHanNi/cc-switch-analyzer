<template>
  <div class="filter-bar">
    <div class="filter-row">
      <div class="filter-group">
        <span class="filter-label">日期范围</span>
        <n-date-picker
          v-model:value="dateRange"
          type="daterange"
          size="small"
          clearable
          style="width: 240px"
        />
      </div>

      <div class="filter-group">
        <span class="filter-label">供应商</span>
        <n-select
          v-model:value="filterStore.providerId"
          :options="providerSelectOptions"
          size="small"
          clearable
          style="width: 150px"
          placeholder="全部"
        />
      </div>

      <div class="filter-group">
        <span class="filter-label">模型</span>
        <n-select
          v-model:value="filterStore.modelId"
          :options="modelSelectOptions"
          size="small"
          clearable
          style="width: 150px"
          placeholder="全部"
        />
      </div>

      <div class="filter-actions">
        <n-button size="small" type="primary" :disabled="!dbStore.hasDatabase" @click="onQuery">
          查询
        </n-button>
        <n-button size="small" :disabled="!dbStore.hasDatabase" @click="onReset">
          重置
        </n-button>
      </div>
    </div>

    <div class="quick-dates">
      <n-button
        v-for="btn in quickDateButtons"
        :key="btn.days"
        size="tiny"
        :type="activeQuickDays === btn.days ? 'primary' : 'default'"
        :disabled="!dbStore.hasDatabase"
        @click="onQuickDate(btn.days)"
      >
        {{ btn.label }}
      </n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { NButton, NDatePicker, NSelect } from 'naive-ui'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useFilter } from '@/composables/useFilter'

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const { executeQuery, quickDateQuery } = useFilter()

// 当前激活的快捷日期天数
const activeQuickDays = ref<number | null>(30)

// 日期范围（Naive UI 使用时间戳范围）
const dateRange = ref<[number, number] | null>(null)

// dateRange → filterStore
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

// filterStore → dateRange（初始化 + 外部设置）
watch([() => filterStore.fromDate, () => filterStore.toDate], ([f, t]) => {
  if (f && t) {
    const ts = [f.getTime(), t.getTime()] as [number, number]
    // 避免循环更新
    if (!dateRange.value || dateRange.value[0] !== ts[0] || dateRange.value[1] !== ts[1]) {
      dateRange.value = ts
    }
  }
}, { immediate: true })

const quickDateButtons = [
  { label: '近 1 天', days: 1 },
  { label: '近 7 天', days: 7 },
  { label: '近 30 天', days: 30 },
  { label: '近 60 天', days: 60 },
  { label: '近 180 天', days: 180 }
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
  await executeQuery()
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
  padding: 4px 12px 8px;
  border-top: 1px solid var(--border-light);
}

.filter-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.filter-group {
  display: flex;
  align-items: center;
  gap: 4px;
}

.filter-label {
  font-size: 12px;
  color: var(--text-tertiary);
  white-space: nowrap;
}

.filter-actions {
  display: flex;
  gap: 6px;
}

.quick-dates {
  display: flex;
  gap: 2px;
  margin-top: 4px;
}
</style>
