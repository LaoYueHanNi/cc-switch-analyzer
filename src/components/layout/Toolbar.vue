<template>
  <div class="toolbar">
    <div class="toolbar-left">
      <n-button size="tiny" type="primary" @click="onSelectDb">
        <template #icon>
          <n-icon size="14"><folder-open-outline /></n-icon>
        </template>
        选择数据库
      </n-button>
      <span class="db-path" v-if="dbStore.hasDatabase">
        {{ dbStore.dbPath }} | 共 {{ dbStore.recordCount }} 条记录
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { NButton, NIcon } from 'naive-ui'
import { FolderOpenOutline } from '@vicons/ionicons5'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'
const dbStore = useDatabaseStore()
const { selectDatabase } = useDatabase()

async function onSelectDb(): Promise<void> {
  await selectDatabase()
}
</script>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 12px;
  gap: 8px;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.db-path {
  font-size: 12px;
  color: var(--text-tertiary);
  max-width: 500px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
