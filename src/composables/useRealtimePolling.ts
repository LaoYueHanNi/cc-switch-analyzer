import { ref, onUnmounted } from 'vue'
import { platformAdapter } from '@/platform'
import type { RealtimeRequestLog } from '@/types/database'

const MAX_LOGS = 500

// 增量合并去重指纹。后端游标是 >= 语义（保证同秒后到的记录不被漏掉），会重复
// 返回游标秒内已见记录，靠此指纹集兜底不重复追加。dbType/providerId/sessionId
// 防跨源、跨会话误判；latencyMs 区分同秒同 token 的真实不同请求。
function logKey(r: RealtimeRequestLog): string {
  return [
    r.dbType, r.providerId, r.sessionId, r.createdAt, r.model,
    r.inputTokens, r.outputTokens, r.cacheReadTokens, r.cacheCreationTokens, r.latencyMs,
  ].join('|')
}

export function useRealtimePolling() {
  const logs = ref<RealtimeRequestLog[]>([])
  const lastRefreshTime = ref('')
  const isPolling = ref(false)
  let isFetchingBusy = false
  let timer: ReturnType<typeof setInterval> | null = null
  let prevMaxCreatedAt = 0
  // 已见记录指纹集。随 logs 的 MAX_LOGS 截断而不收缩：游标单调递增，历史指纹
  // 不会再命中，仅占少量内存，保留可避免列表滚动边界的重复闪现。
  let seenKeys = new Set<string>()

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
        seenKeys = new Set(data.map(logKey))
        logs.value = data
      } else {
        // 增量:since 之后的新记录同样有序返回,fresh[0] 为最新,游标单调推进
        const fresh: RealtimeRequestLog[] = await platformAdapter.queryRealtimeLogs(prevMaxCreatedAt) ?? []
        const newRows = fresh.filter(r => !seenKeys.has(logKey(r)))
        if (newRows.length > 0) {
          for (const row of newRows) {
            seenKeys.add(logKey(row))
            if (!silent) {
              row.isNew = true
            }
          }
          logs.value = [...newRows, ...logs.value].slice(0, MAX_LOGS)
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
    seenKeys = new Set()
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
