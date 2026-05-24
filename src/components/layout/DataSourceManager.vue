<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)" preset="card" title="数据源管理" size="small" style="max-width: 500px">
    <div class="source-list">
      <div class="source-item" v-for="slot in slots" :key="slot.key">
        <button class="source-type" :class="slot.key" @click="onSelect(slot.key)">
          {{ slot.label }}
        </button>
        <span class="source-path" :title="slot.path || ''">{{ slot.path || '未选择' }}</span>
        <button v-if="slot.path" class="remove-btn" @click="onRemove(slot.key)" title="移除">
          <n-icon size="12"><close-outline /></n-icon>
        </button>
      </div>
    </div>

    <!-- TrafficMonitor 插件管理 -->
    <n-divider style="margin: 12px 0 8px" />
    <div class="tm-section">
      <div class="tm-header">TrafficMonitor 插件管理</div>

      <div class="tm-row">
        <n-button size="tiny" :loading="downloading === 'x86'" @click="downloadPlugin('x86')">
          下载 x86 插件
        </n-button>
        <n-button size="tiny" :loading="downloading === 'x64'" @click="downloadPlugin('x64')">
          下载 x64 插件
        </n-button>
        <span class="tm-hint" v-if="downloadedPath">
          已下载至 {{ downloadedPath }}
        </span>
      </div>

      <div class="tm-row">
        <n-switch :value="tmStatus.enabled" @update:value="toggleService" size="small" />
        <span class="tm-label">启用服务</span>
        <span class="tm-hint" v-if="tmStatus.running">
          已启用 · 端口 {{ tmStatus.port }}
        </span>
      </div>
    </div>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { NModal, NIcon, NButton, NSwitch, NDivider } from 'naive-ui'
import { CloseOutline } from '@vicons/ionicons5'
import { invoke } from '@tauri-apps/api/core'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'
import { platformAdapter } from '@/platform'

defineProps<{ show: boolean }>()
defineEmits<{ 'update:show': [value: boolean] }>()

const dbStore = useDatabaseStore()
const { addDatabase, removeDatabase } = useDatabase()

interface DefaultPaths { ccSwitch: string | null; opencode: string | null; aiProxy: string | null }
const defaultPaths = ref<DefaultPaths>({ ccSwitch: null, opencode: null, aiProxy: null })

async function loadDefaultPaths(): Promise<void> {
  try {
    defaultPaths.value = await invoke<DefaultPaths>('get_default_paths')
  } catch { /* ignore */ }
}
loadDefaultPaths()

const slots = computed(() => [
  {
    key: 'cc-switch',
    label: 'CC-Switch',
    path: dbStore.sources.find(s => s.dbType === 'CC-Switch')?.path || '',
    defaultPath: defaultPaths.value.ccSwitch,
  },
  {
    key: 'opencode',
    label: 'OpenCode',
    path: dbStore.sources.find(s => s.dbType === 'OpenCode')?.path || '',
    defaultPath: defaultPaths.value.opencode,
  },
  {
    key: 'ai-proxy',
    label: 'AI-Proxy',
    path: dbStore.sources.find(s => s.dbType === 'AI-Proxy')?.path || '',
    defaultPath: defaultPaths.value.aiProxy,
  },
])

async function onSelect(key: string): Promise<void> {
  const slot = slots.value.find(s => s.key === key)
  const filePath = await platformAdapter.pickDatabaseFile(slot?.defaultPath || undefined)
  if (!filePath) return
  const dbType = key === 'cc-switch' ? 'CC-Switch' : key === 'opencode' ? 'OpenCode' : 'AI-Proxy'
  const existing = dbStore.sources.find(s => s.dbType === dbType)
  if (existing) {
    await removeDatabase(existing.id)
  }
  await addDatabase(filePath)
}

async function onRemove(key: string): Promise<void> {
  const dbType = key === 'cc-switch' ? 'CC-Switch' : key === 'opencode' ? 'OpenCode' : 'AI-Proxy'
  const src = dbStore.sources.find(s => s.dbType === dbType)
  if (src) {
    await removeDatabase(src.id)
  }
}

// ===== TrafficMonitor 插件管理 =====

interface TmServiceStatus {
  enabled: boolean
  running: boolean
  port: number
}

const tmStatus = ref<TmServiceStatus>({ enabled: false, running: false, port: 19810 })
const downloading = ref<string | false>(false)
const downloadedPath = ref('')

async function loadTmStatus(): Promise<void> {
  try {
    tmStatus.value = await invoke<TmServiceStatus>('get_http_service_status')
  } catch { /* ignore */ }
}
loadTmStatus()

async function downloadPlugin(arch: 'x86' | 'x64'): Promise<void> {
  downloading.value = arch
  try {
    const path = await invoke<string>('download_traffic_monitor_plugin', { arch })
    downloadedPath.value = path
  } catch {
    // ignore
  } finally {
    downloading.value = false
  }
}

async function toggleService(enabled: boolean): Promise<void> {
  try {
    tmStatus.value = await invoke<TmServiceStatus>('toggle_http_service', { enabled })
  } catch {
    // ignore
  }
}
</script>

<style scoped>
.source-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.source-item {
  display: flex;
  align-items: center;
  gap: 8px;
}

.source-type {
  font-size: 10px;
  padding: 3px 8px;
  border: none;
  border-radius: 3px;
  font-weight: 500;
  white-space: nowrap;
  cursor: pointer;
  line-height: 1.4;
}

.source-type.cc-switch {
  background: var(--color-blue);
  color: #fff;
}

.source-type.cc-switch:hover {
  opacity: 0.85;
}

.source-type.opencode {
  background: var(--color-amber);
  color: #fff;
}

.source-type.opencode:hover {
  opacity: 0.85;
}

.source-type.ai-proxy {
  background: var(--color-green);
  color: #fff;
}

.source-type.ai-proxy:hover {
  opacity: 0.85;
}

.source-path {
  flex: 1;
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.remove-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
}

.remove-btn:hover {
  background: var(--border-light);
  color: var(--color-cost);
}

/* TrafficMonitor 插件管理 */
.tm-section {
  padding: 4px 0;
}

.tm-header {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 8px;
}

.tm-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}

.tm-label {
  font-size: 12px;
  color: var(--text-secondary);
}

.tm-hint {
  font-size: 11px;
  color: var(--text-tertiary);
}
</style>
