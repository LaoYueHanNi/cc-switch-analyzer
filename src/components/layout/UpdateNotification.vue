<template>
  <Teleport to="body">
    <div v-if="show" class="update-overlay" @click.self="updaterStore.dismiss()">
      <div class="update-dialog">
        <!-- 发现更新 -->
        <template v-if="updaterStore.status === 'available'">
          <div class="update-title">发现新版本 v{{ updaterStore.updateInfo?.version }}</div>
          <div class="update-sub">当前版本 v{{ updaterStore.updateInfo?.currentVersion }}</div>
          <div v-if="updaterStore.updateInfo?.body" class="update-notes">{{ updaterStore.updateInfo.body }}</div>
          <div class="update-actions">
            <button class="update-btn primary" @click="updaterStore.downloadAndInstall()">立即更新</button>
            <button class="update-btn" @click="updaterStore.dismiss()">跳过</button>
          </div>
        </template>

        <!-- 下载中 -->
        <template v-else-if="updaterStore.status === 'downloading'">
          <div class="update-title">正在下载更新...</div>
          <div class="update-progress">
            <div class="progress-bar" :style="{ width: '30%' }"></div>
          </div>
          <div class="update-size">{{ formatSize(updaterStore.downloadedBytes) }}</div>
        </template>

        <!-- 错误 -->
        <template v-else-if="updaterStore.status === 'error'">
          <div class="update-title">更新失败</div>
          <div class="update-error">{{ updaterStore.errorMessage }}</div>
          <div class="update-actions">
            <button class="update-btn primary" @click="updaterStore.downloadAndInstall()">重试</button>
            <button class="update-btn" @click="updaterStore.dismiss()">关闭</button>
          </div>
        </template>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useUpdaterStore } from '@/stores/updater'

const updaterStore = useUpdaterStore()

const show = computed(() =>
  updaterStore.status === 'available' || updaterStore.status === 'downloading' || updaterStore.status === 'error'
)

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + ' B'
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB'
  return (bytes / (1024 * 1024)).toFixed(1) + ' MB'
}
</script>

<style scoped>
.update-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
}

.update-dialog {
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 8px;
  padding: 20px 24px;
  min-width: 320px;
  max-width: 420px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
}

.update-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 4px;
}

.update-sub {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 12px;
}

.update-notes {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
  max-height: 120px;
  overflow-y: auto;
  padding: 8px;
  background: var(--bg-hover);
  border-radius: 4px;
  margin-bottom: 16px;
  white-space: pre-wrap;
}

.update-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  margin-top: 16px;
}

.update-btn {
  font-size: 12px;
  padding: 5px 16px;
  border: 1px solid var(--border-main);
  border-radius: 4px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
}

.update-btn:hover {
  border-color: var(--color-blue);
  color: var(--color-blue);
}

.update-btn.primary {
  background: var(--color-blue);
  border-color: var(--color-blue);
  color: #fff;
}

.update-btn.primary:hover {
  opacity: 0.85;
}

.update-progress {
  height: 4px;
  background: var(--bg-hover);
  border-radius: 2px;
  margin: 12px 0 6px;
  overflow: hidden;
}

.progress-bar {
  height: 100%;
  background: var(--color-blue);
  border-radius: 2px;
  transition: width 0.3s;
}

.update-size {
  font-size: 11px;
  color: var(--text-muted);
}

.update-error {
  font-size: 12px;
  color: var(--color-cost);
  margin-top: 8px;
}
</style>
