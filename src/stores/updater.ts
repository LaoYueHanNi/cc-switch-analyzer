import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { platformAdapter } from '@/platform'
import type { UpdateInfo } from '@/platform/types'

export type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'error'

export const useUpdaterStore = defineStore('updater', () => {
  const status = ref<UpdateStatus>('idle')
  const updateInfo = ref<UpdateInfo | null>(null)
  const downloadedBytes = ref(0)
  const errorMessage = ref('')

  async function checkForUpdate(): Promise<void> {
    if (status.value === 'checking' || status.value === 'downloading') return
    status.value = 'checking'
    try {
      const info = await platformAdapter.checkForUpdate()
      if (info) {
        updateInfo.value = info
        status.value = 'available'
      } else {
        status.value = 'idle'
      }
    } catch (e: any) {
      status.value = 'error'
      errorMessage.value = e.message || '检查更新失败'
    }
  }

  async function downloadAndInstall(): Promise<void> {
    if (!updateInfo.value) return
    status.value = 'downloading'
    downloadedBytes.value = 0
    try {
      await platformAdapter.downloadAndInstall((downloaded) => {
        downloadedBytes.value = downloaded
      })
      status.value = 'idle'
    } catch (e: any) {
      status.value = 'error'
      errorMessage.value = e.message || '下载更新失败'
    }
  }

  function dismiss(): void {
    status.value = 'idle'
    updateInfo.value = null
    errorMessage.value = ''
    downloadedBytes.value = 0
  }

  return {
    status: computed(() => status.value),
    updateInfo: computed(() => updateInfo.value),
    downloadedBytes: computed(() => downloadedBytes.value),
    errorMessage: computed(() => errorMessage.value),
    checkForUpdate,
    downloadAndInstall,
    dismiss,
  }
})
