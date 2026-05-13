import { ref, onUnmounted } from 'vue'
import { platformAdapter } from '@/platform'
import type { RealtimeRequestLog } from '@/types/database'

const MAX_LOGS = 500

export function useRealtimePolling() {
  const logs = ref<RealtimeRequestLog[]>([])
  const lastRefreshTime = ref('')
  const isPolling = ref(false)
  let isFetchingBusy = false
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
    if (isFetchingBusy) return
    isFetchingBusy = true
    try {
      if (prevMaxCreatedAt === 0) {
        // 首次：全量加载
        const data: RealtimeRequestLog[] = await platformAdapter.queryRealtimeLogs() ?? []
        if (data.length > 0) {
          prevMaxCreatedAt = data[0].createdAt
        }
        logs.value = data
      } else {
        // 增量：只查 created_at > prevMax 的新行
        const fresh: RealtimeRequestLog[] = await platformAdapter.queryRealtimeLogs(prevMaxCreatedAt) ?? []
        if (fresh.length > 0) {
          for (const row of fresh) {
            row.isNew = true
          }
          logs.value = [...fresh, ...logs.value].slice(0, MAX_LOGS)
          prevMaxCreatedAt = fresh[0].createdAt
        }
      }
      lastRefreshTime.value = new Date().toLocaleTimeString('zh-CN')
    } catch (err: any) {
      console.error('实时日志查询失败:', err)
    } finally {
      isFetchingBusy = false
    }
  }

  async function refreshNow(): Promise<void> {
    // 强制全量刷新
    prevMaxCreatedAt = 0
    logs.value = []
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
