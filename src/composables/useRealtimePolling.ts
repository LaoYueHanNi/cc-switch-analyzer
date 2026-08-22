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

  function startPolling(silentFirst: boolean = false): void {
    if (isPolling.value) return
    isPolling.value = true
    fetchData(silentFirst)
    timer = setInterval(() => fetchData(false), 10_000)
  }

  function stopPolling(): void {
    isPolling.value = false
    if (timer) {
      clearInterval(timer)
      timer = null
    }
  }

  async function fetchData(silent: boolean = false): Promise<void> {
    if (isFetchingBusy) return
    isFetchingBusy = true
    try {
      if (prevMaxCreatedAt === 0) {
        // 首屏:后端按 created_at 全局倒序并截断 500,data[0] 即全局最新,作为增量游标
        const data: RealtimeRequestLog[] = await platformAdapter.queryRealtimeLogs() ?? []
        if (data.length > 0) {
          prevMaxCreatedAt = data[0].createdAt
        }
        logs.value = data
      } else {
        // 增量:since 之后的新记录同样有序返回,fresh[0] 为最新,游标单调推进
        const fresh: RealtimeRequestLog[] = await platformAdapter.queryRealtimeLogs(prevMaxCreatedAt) ?? []
        if (fresh.length > 0) {
          if (!silent) {
            for (const row of fresh) {
              row.isNew = true
            }
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
    prevMaxCreatedAt = 0
    logs.value = []
    await fetchData(false)
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
