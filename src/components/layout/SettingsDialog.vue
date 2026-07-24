<template>
  <n-modal
    :show="show"
    @update:show="$emit('update:show', $event)"
    preset="card"
    title="设置"
    :style="{ width: '780px', maxWidth: '92vw' }"
    :content-style="{ padding: 0 }"
    :trap-focus="false"
  >
    <div class="settings-layout">
      <nav class="settings-nav">
        <button
          v-for="item in tabs"
          :key="item.key"
          type="button"
          class="settings-nav-item"
          :class="{ active: activeTab === item.key }"
          @click="activeTab = item.key"
        >
          <n-icon size="16"><component :is="item.icon" /></n-icon>
          <span>{{ item.label }}</span>
        </button>
      </nav>
      <div class="settings-panel">
        <SettingsDataSources v-if="activeTab === 'sources'" :active="show && activeTab === 'sources'" />
        <SettingsPlugin v-else-if="activeTab === 'plugin'" :active="show && activeTab === 'plugin'" />
        <SettingsUpdate v-else-if="activeTab === 'update'" />
      </div>
    </div>
  </n-modal>
</template>

<script setup lang="ts">
import { ref, type Component } from 'vue'
import { NModal, NIcon } from 'naive-ui'
import { ServerOutline, ExtensionPuzzleOutline, CloudDownloadOutline } from '@vicons/ionicons5'
import SettingsDataSources from './settings/SettingsDataSources.vue'
import SettingsPlugin from './settings/SettingsPlugin.vue'
import SettingsUpdate from './settings/SettingsUpdate.vue'

defineProps<{ show: boolean }>()
defineEmits<{ 'update:show': [value: boolean] }>()

type TabKey = 'sources' | 'plugin' | 'update'

const activeTab = ref<TabKey>('sources')

const tabs: { key: TabKey; label: string; icon: Component }[] = [
  { key: 'sources', label: '数据源', icon: ServerOutline },
  { key: 'plugin', label: '插件管理', icon: ExtensionPuzzleOutline },
  { key: 'update', label: '版本更新', icon: CloudDownloadOutline },
]
</script>

<style scoped>
.settings-layout {
  display: flex;
  height: min(72vh, 560px);
  min-height: 360px;
}

.settings-nav {
  width: 140px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px 8px;
  border-right: 1px solid var(--border-main);
  background: var(--bg-base);
}

.settings-nav-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
  font-size: 12px;
  cursor: pointer;
  text-align: left;
  position: relative;
  transition: background var(--transition-speed), color var(--transition-speed);
}

.settings-nav-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.settings-nav-item.active {
  background: var(--bg-hover);
  color: var(--color-blue);
  font-weight: 500;
}

.settings-nav-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 6px;
  bottom: 6px;
  width: 3px;
  border-radius: 2px;
  background: var(--color-blue);
}

.settings-panel {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  padding: 16px 20px;
}
</style>
