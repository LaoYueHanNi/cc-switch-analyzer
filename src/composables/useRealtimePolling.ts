import { ref, onUnmounted } from 'vue'
import { platformAdapter } from '@/platform'
import type { RealtimeBucket } from '@/types/database'

// 实时轮询 composable
export function useRealtimePolling() {
  const buckets = ref<RealtimeBucket[]>([])
  const lastRefreshTime = ref('')
  const isPolling = ref(false)
  let timer: ReturnType<typeof setInterval> | null = null

  function startPolling(): void {
    if (isPolling.value) return
    isPolling.value = true
    fetchData()
    timer = setInterval(fetchData, 10_000)
  }

  function stopPolling(): void {
    isPolling.value = false
    if (timer) {
      clearInterval(timer)
      timer = null
    }
  }

  async function fetchData(): Promise<void> {
    try {
      const result = await platformAdapter.queryRealtime()
      buckets.value = result || []
      lastRefreshTime.value = new Date().toLocaleTimeString('zh-CN')
    } catch (err: any) {
      console.error('实时数据查询失败:', err)
    }
  }

  async function refreshNow(): Promise<void> {
    await fetchData()
  }

  onUnmounted(stopPolling)

  return {
    buckets,
    lastRefreshTime,
    isPolling,
    startPolling,
    stopPolling,
    refreshNow
  }
}
