<template>
  <div class="toolbar">
    <div class="toolbar-left">
      <button class="toolbar-btn" :disabled="!dbStore.hasDatabase" @click="showManager = true">
        <n-icon size="14"><settings-outline /></n-icon>
      </button>
      <span class="db-info" v-if="dbStore.hasDatabase">
        {{ dbStore.sources.length }} 个数据源 · {{ dbStore.recordCount }} 条记录
      </span>
      <span
        v-if="hookAlert"
        class="hook-alert"
        :title="hookAlert.message"
        @click="refreshHookAlert"
      >
        <span class="hook-alert-dot"></span>
        <span class="hook-alert-text">Hook 异常</span>
      </span>
    </div>
    <SettingsDialog v-model:show="showManager" />
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { NIcon } from 'naive-ui'
import { SettingsOutline } from '@vicons/ionicons5'
import { useDatabaseStore } from '@/stores/database'
import { platformAdapter } from '@/platform'
import type { CursorStatusInfo } from '@/platform/types'
import SettingsDialog from './SettingsDialog.vue'

const dbStore = useDatabaseStore()
const showManager = ref(false)
const hookAlert = ref<CursorStatusInfo['hookAlert']>(null)

async function refreshHookAlert(): Promise<void> {
  try {
    const status = await platformAdapter.cursorStatus()
    hookAlert.value = status.hookAlert ?? null
  } catch (e) {
    console.error('[toolbar] refresh hook alert failed:', e)
  }
}

let interval: ReturnType<typeof setInterval> | null = null

function restartPolling(): void {
  if (interval) clearInterval(interval)
  // Alert present: poll faster so recovery clears the red light quickly
  const ms = hookAlert.value ? 5000 : 30000
  interval = setInterval(refreshHookAlert, ms)
}

onMounted(() => {
  refreshHookAlert().finally(restartPolling)
  window.addEventListener('focus', refreshHookAlert)
})

onUnmounted(() => {
  if (interval) clearInterval(interval)
  window.removeEventListener('focus', refreshHookAlert)
})

watch(hookAlert, () => restartPolling())
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
  cursor: not-allowed;
}

.db-info {
  font-size: 12px;
  color: var(--text-tertiary);
}

.hook-alert {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: #d63031;
  cursor: pointer;
  margin-left: 4px;
}

.hook-alert-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #d63031;
  box-shadow: 0 0 0 2px color-mix(in srgb, #d63031 30%, transparent);
}

.hook-alert-text {
  white-space: nowrap;
}
</style>
