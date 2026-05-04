import { ref, onUnmounted } from 'vue'
import { platformAdapter } from '@/platform'
import type { RealtimeRequestLog } from '@/types/database'

// 实时轮询 composable
export function useRealtimePolling() {
  const logs = ref<RealtimeRequestLog[]>([])
  const lastRefreshTime = ref('')
  const isPolling = ref(false)
  let timer: ReturnType<typeof setInterval> | null = null
  let prevMaxCreatedAt = 0

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
      const result = await platformAdapter.queryRealtimeLogs()
      const data = result || []
      // 标记上次最大时间戳（下次刷新用）
      if (data.length > 0) {
        const currentMax = data[0].createdAt
        if (prevMaxCreatedAt === 0) prevMaxCreatedAt = currentMax
        // 给新行打标
        for (const row of data) {
          if (row.createdAt > prevMaxCreatedAt) {
            (row as any)._new = true
          }
        }
        prevMaxCreatedAt = currentMax
      }
      logs.value = data
      lastRefreshTime.value = new Date().toLocaleTimeString('zh-CN')
    } catch (err: any) {
      console.error('实时日志查询失败:', err)
    }
  }

  async function refreshNow(): Promise<void> {
    await fetchData()
  }

  onUnmounted(stopPolling)

  return {
    logs,
    lastRefreshTime,
    isPolling,
    startPolling,
    stopPolling,
    refreshNow
  }
}
