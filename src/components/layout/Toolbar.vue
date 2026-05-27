<template>
  <div class="toolbar">
    <div class="toolbar-left">
      <button class="toolbar-btn" :disabled="!dbStore.hasDatabase" @click="showManager = true">
        <n-icon size="14"><settings-outline /></n-icon>
      </button>
      <button v-if="updaterStore.status === 'available'" class="toolbar-btn update-badge" @click="updaterStore.downloadAndInstall()" title="有新版本可用">
        <n-icon size="14"><cloud-download-outline /></n-icon>
      </button>
      <span class="db-info" v-if="dbStore.hasDatabase">
        {{ dbStore.sources.length }} 个数据源 · {{ dbStore.recordCount }} 条记录
      </span>
    </div>
    <DataSourceManager v-model:show="showManager" />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { NIcon } from 'naive-ui'
import { SettingsOutline, CloudDownloadOutline } from '@vicons/ionicons5'
import { useDatabaseStore } from '@/stores/database'
import { useUpdaterStore } from '@/stores/updater'
import DataSourceManager from './DataSourceManager.vue'

const dbStore = useDatabaseStore()
const updaterStore = useUpdaterStore()
const showManager = ref(false)
</script>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 12px;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.toolbar-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 24px;
  border: 1px solid var(--border-main);
  border-radius: 4px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.toolbar-btn:hover:not(:disabled) {
  border-color: var(--color-blue);
  color: var(--color-blue);
}

.toolbar-btn:disabled {
  opacity: 0.4;
  cursor: not-disabled;
}

.update-badge {
  border-color: var(--color-green);
  color: var(--color-green);
}

.db-info {
  font-size: 12px;
  color: var(--text-tertiary);
}
</style>
