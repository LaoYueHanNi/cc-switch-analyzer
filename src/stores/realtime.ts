import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { RealtimeBucket } from '@/types/database'

// 实时 Token 监控状态
export const useRealtimeStore = defineStore('realtime', () => {
  const buckets = ref<RealtimeBucket[]>([])
  const isPolling = ref(false)
  const lastRefreshTime = ref('')

  return {
    buckets,
    isPolling,
    lastRefreshTime
  }
})
