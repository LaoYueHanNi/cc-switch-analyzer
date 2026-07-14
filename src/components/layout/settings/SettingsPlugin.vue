<template>
  <div class="tm-section">
    <div class="tm-header">TrafficMonitor 插件</div>
    <p class="tm-desc">下载插件 DLL 并启用本地 HTTP 服务，供 TrafficMonitor 展示今日 Token 与费用。</p>

    <div class="tm-row">
      <n-button size="tiny" :loading="downloading === 'x86'" @click="downloadPlugin('x86')">
        下载 x86 插件
      </n-button>
      <n-button size="tiny" :loading="downloading === 'x64'" @click="downloadPlugin('x64')">
        下载 x64 插件
      </n-button>
    </div>
    <p class="tm-hint path-hint" v-if="downloadedPath">
      已下载至 {{ downloadedPath }}
    </p>

    <div class="tm-row service-row">
      <n-switch :value="tmStatus.enabled" @update:value="toggleService" size="small" />
      <span class="tm-label">启用服务</span>
      <span class="tm-hint" v-if="tmStatus.running">
        已启用 · 端口 {{ tmStatus.port }}
      </span>
      <span class="tm-hint" v-else-if="!tmStatus.enabled">
        未启用
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { NButton, NSwitch } from 'naive-ui'
import { platformAdapter } from '@/platform'
import type { TmServiceStatus } from '@/platform/types'

const props = defineProps<{ active: boolean }>()

const tmStatus = ref<TmServiceStatus>({ enabled: false, running: false, port: 19810 })
const downloading = ref<string | false>(false)
const downloadedPath = ref('')

async function loadTmStatus(): Promise<void> {
  try {
    tmStatus.value = await platformAdapter.getHttpServiceStatus()
  } catch { /* ignore */ }
}

watch(
  () => props.active,
  (visible) => {
    if (visible) loadTmStatus()
  },
  { immediate: true },
)

async function downloadPlugin(arch: 'x86' | 'x64'): Promise<void> {
  downloading.value = arch
  try {
    const path = await platformAdapter.downloadTrafficMonitorPlugin(arch)
    downloadedPath.value = path
  } catch {
    // ignore
  } finally {
    downloading.value = false
  }
}

async function toggleService(enabled: boolean): Promise<void> {
  try {
    tmStatus.value = await platformAdapter.toggleHttpService(enabled)
  } catch {
    // ignore
  }
}
</script>

<style scoped>
.tm-section {
  padding: 0;
}

.tm-header {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 6px;
}

.tm-desc {
  font-size: 12px;
  color: var(--text-tertiary);
  line-height: 1.5;
  margin: 0 0 16px;
}

.tm-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
  flex-wrap: wrap;
}

.service-row {
  margin-top: 8px;
  padding-top: 14px;
  border-top: 1px solid var(--border-main);
}

.tm-label {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.tm-hint {
  font-size: 11px;
  color: var(--text-tertiary);
}

.path-hint {
  margin: -4px 0 8px;
  word-break: break-all;
}
</style>
