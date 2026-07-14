<template>
  <div class="tm-section">
    <div class="tm-header">版本更新</div>
    <p class="tm-desc">检查并下载应用新版本。若网络受限，可配置 HTTP 代理。</p>

    <div class="tm-row">
      <button class="check-update-btn" :disabled="updaterStore.status === 'checking'" @click="updaterStore.checkForUpdate()">
        {{ updaterStore.status === 'checking' ? '检查中...' : '检查更新' }}
      </button>
      <span class="tm-hint" v-if="updaterStore.status === 'idle'">当前版本 v{{ currentVersion }}</span>
      <span class="tm-hint up-to-date" v-if="updaterStore.status === 'upToDate'">已是最新版本</span>
    </div>

    <div class="tm-row proxy-row">
      <span class="proxy-label">代理</span>
      <input
        class="proxy-input"
        :value="updaterStore.proxy"
        @input="updaterStore.setProxy(($event.target as HTMLInputElement).value)"
        placeholder="http://127.0.0.1:7890"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useUpdaterStore } from '@/stores/updater'
import { platformAdapter } from '@/platform'

const updaterStore = useUpdaterStore()
const currentVersion = ref('')
platformAdapter.getAppVersion().then(v => currentVersion.value = v).catch(() => {})
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
  margin-bottom: 12px;
  flex-wrap: wrap;
}

.proxy-row {
  margin-top: 4px;
}

.tm-hint {
  font-size: 11px;
  color: var(--text-tertiary);
}

.check-update-btn {
  font-size: 11px;
  padding: 3px 12px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  line-height: 1.4;
}

.check-update-btn:hover:not(:disabled) {
  border-color: var(--color-blue);
  color: var(--color-blue);
}

.check-update-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.up-to-date {
  color: var(--color-green);
}

.proxy-label {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.proxy-input {
  flex: 1;
  min-width: 180px;
  font-size: 11px;
  padding: 4px 8px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: var(--bg-card);
  color: var(--text-primary);
  outline: none;
}

.proxy-input:focus {
  border-color: var(--color-blue);
}

.proxy-input::placeholder {
  color: var(--text-faint);
}
</style>
