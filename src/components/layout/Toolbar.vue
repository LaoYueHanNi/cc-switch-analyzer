<template>
  <div class="toolbar">
    <div class="toolbar-left">
      <n-button size="small" type="primary" @click="onSelectDb">
        <template #icon>
          <n-icon><folder-open-outline /></n-icon>
        </template>
        选择数据库
      </n-button>
      <span class="db-path" v-if="dbStore.hasDatabase">
        {{ dbStore.dbPath }} | 共 {{ dbStore.recordCount }} 条记录
      </span>
    </div>
    <div class="toolbar-right">
      <n-button size="small" :disabled="!dbStore.hasDatabase" @click="onRefresh">
        刷新
      </n-button>
      <n-select
        v-model:value="refreshInterval"
        :options="intervalOptions"
        size="small"
        style="width: 100px"
        :disabled="!dbStore.hasDatabase"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { NButton, NSelect, NIcon } from 'naive-ui'
import { FolderOpenOutline } from '@vicons/ionicons5'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'

const dbStore = useDatabaseStore()
const { selectDatabase, refreshDatabase } = useDatabase()
const refreshInterval = ref('manual')

const intervalOptions = [
  { label: '手动', value: 'manual' },
  { label: '30秒', value: '30s' },
  { label: '1分钟', value: '1min' },
  { label: '5分钟', value: '5min' },
  { label: '30分钟', value: '30min' }
]

async function onSelectDb(): Promise<void> {
  await selectDatabase()
}

async function onRefresh(): Promise<void> {
  await refreshDatabase()
}

// 自动刷新定时器
let autoRefreshTimer: ReturnType<typeof setInterval> | null = null

watch(refreshInterval, (val) => {
  if (autoRefreshTimer) {
    clearInterval(autoRefreshTimer)
    autoRefreshTimer = null
  }

  const intervalMap: Record<string, number> = {
    '30s': 30_000,
    '1min': 60_000,
    '5min': 300_000,
    '30min': 1_800_000
  }

  const ms = intervalMap[val]
  if (ms) {
    autoRefreshTimer = setInterval(() => {
      if (dbStore.hasDatabase) refreshDatabase()
    }, ms)
  }
})
</script>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  gap: 12px;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.db-path {
  font-size: 12px;
  color: #666;
  max-width: 500px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
