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
  </n-modal>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { NModal, NIcon } from 'naive-ui'
import { CloseOutline } from '@vicons/ionicons5'
import { invoke } from '@tauri-apps/api/core'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'
import { platformAdapter } from '@/platform'

defineProps<{ show: boolean }>()
defineEmits<{ 'update:show': [value: boolean] }>()

const dbStore = useDatabaseStore()
const { addDatabase, removeDatabase } = useDatabase()

interface DefaultPaths { ccSwitch: string | null; opencode: string | null }
const defaultPaths = ref<DefaultPaths>({ ccSwitch: null, opencode: null })

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
])

async function onSelect(key: string): Promise<void> {
  const slot = slots.value.find(s => s.key === key)
  const filePath = await platformAdapter.pickDatabaseFile(slot?.defaultPath || undefined)
  if (!filePath) return
  const dbType = key === 'cc-switch' ? 'CC-Switch' : 'OpenCode'
  const existing = dbStore.sources.find(s => s.dbType === dbType)
  if (existing) {
    await removeDatabase(existing.id)
  }
  await addDatabase(filePath)
}

async function onRemove(key: string): Promise<void> {
  const dbType = key === 'cc-switch' ? 'CC-Switch' : 'OpenCode'
  const src = dbStore.sources.find(s => s.dbType === dbType)
  if (src) {
    await removeDatabase(src.id)
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
</style>
