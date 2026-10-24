import { ref, onUnmounted } from 'vue'
import { platformAdapter } from '@/platform'
import type { RealtimeRequestLog } from '@/types/database'

const MAX_LOGS = 500

// 增量合并去重指纹。后端游标是 >= 语义（保证同秒后到的记录不被漏掉），会重复
// 返回游标秒内已见记录，靠此指纹集兜底不重复追加。
// 行级主键优先：requestId（session_request_logs 主键）稳定不变，即使记录落库后
// 被 UPDATE（如 DSH 步进补耗时、Magpie 回填字段）导致其他字段变化也能正确判重；
// 为空（源未提供）时退回七元组指纹：dbType/providerId/sourceSessionId 防跨源、
// 跨会话误判；原始 ID 不随 Codex 会话归属映射变化，旧响应回退 sessionId。
// latencyMs 区分同秒同 token 的真实不同请求。
function logKey(r: RealtimeRequestLog): string {
  if (r.requestId) return `id|${r.dbType}|${r.requestId}`
  return [
    r.dbType, r.providerId, r.sourceSessionId ?? r.sessionId, r.createdAt, r.model,
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
        // 首屏:后端按 created_at 全局倒序并截断 500,logs[0] 即全局最新,作为增量游标
        const page = await platformAdapter.queryRealtimeLogs()
        const data = page?.logs ?? []
        if (data.length > 0) {
          prevMaxCreatedAt = data[0].createdAt
        }
        seenKeys = new Set(data.map(logKey))
        logs.value = data
      } else {
        // 增量:since 之后的新记录同样有序返回,fresh[0] 为最新,游标单调推进
        const page = await platformAdapter.queryRealtimeLogs(prevMaxCreatedAt)
        const fresh = page?.logs ?? []
        // 吸收清单:本轮配对成功被端侧吸收的网关行,可能在早前轮次已单独显示
        // (两源扫描轮错峰跨过轮询边界),按 requestId 移除残留行并清其指纹。
        const absorbed = new Set(page?.absorbed ?? [])
        if (absorbed.size > 0) {
          logs.value = logs.value.filter(r => !r.requestId || !absorbed.has(r.requestId))
          for (const key of [...seenKeys]) {
            if (key.startsWith('id|') && absorbed.has(key.split('|')[2])) {
              seenKeys.delete(key)
            }
          }
        }
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
        } else if (absorbed.size > 0) {
          // 无新增但有移除时也推进游标，避免空轮卡住增量节奏
          prevMaxCreatedAt = fresh[0]?.createdAt ?? prevMaxCreatedAt
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
