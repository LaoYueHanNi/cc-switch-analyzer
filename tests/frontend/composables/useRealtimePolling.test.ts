import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { useRealtimePolling } from '@/composables/useRealtimePolling'
import { platformAdapter } from '@/platform'
import type { RealtimeRequestLog } from '@/types/database'

vi.mock('@/platform', () => ({
  platformAdapter: {
    queryRealtimeLogs: vi.fn()
  }
}))

// 工厂：秒级 createdAt + 可选 token/延迟，模拟后端倒序返回的实时记录
function makeLog(overrides: Partial<RealtimeRequestLog> & { createdAt: number }): RealtimeRequestLog {
  return {
    sessionId: '',
    model: 'glm-5',
    providerId: 'ZCode',
    dbType: 'ZCode',
    inputTokens: 100,
    outputTokens: 50,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    latencyMs: 1000,
    inputCost: 0,
    outputCost: 0,
    cacheReadCost: 0,
    cacheCreationCost: 0,
    totalCost: 0,
    ...overrides
  }
}

describe('useRealtimePolling 增量合并去重', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('首屏全量拉取并建立游标', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValue([
      makeLog({ createdAt: 2000 }),
      makeLog({ createdAt: 1000 })
    ])
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    expect(logs.value).toHaveLength(2)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenCalledWith() // 无参首屏
    stopPolling()
  })

  it('增量轮询重复返回已见记录时不追加（>= 游标语义兜底）', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000 })
    ])
    // 下一轮：>= 游标语义重复返回同一条（真实记录 createdAt 为截断秒、毫秒尾数不参与比较）
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000 })
    ])
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    expect(logs.value).toHaveLength(1)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenCalledTimes(2)
    // 第二轮重复记录被指纹去重，列表不增长
    expect(logs.value).toHaveLength(1)
    stopPolling()
  })

  it('游标秒内新记录能追加，已见重复被过滤', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000, outputTokens: 10 })
    ])
    // 同一秒内后到的记录（createdAt 同为 2000）+ 重复的已见记录
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000, outputTokens: 99 }),
      makeLog({ createdAt: 2000, outputTokens: 10 })
    ])
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenCalledTimes(2)
    expect(logs.value).toHaveLength(2)
    expect(logs.value[0].outputTokens).toBe(99) // 新记录在前
    stopPolling()
  })

  it('新秒记录追加且游标推进', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000 })
    ])
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 3000 })
    ])
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(logs.value).toHaveLength(2)
    // 下一轮请求携带推进后的游标 3000
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([])
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenLastCalledWith(3000)
    stopPolling()
  })

  it('增量全为已见记录时游标不回退、列表不变', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000 })
    ])
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000 })
    ])
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    // 第三轮游标仍应为 2000（无新记录时不推进）
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([])
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenLastCalledWith(2000)
    expect(logs.value).toHaveLength(1)
    stopPolling()
  })

  it('refreshNow 清空游标与指纹集重新全量拉取', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000 })
    ])
    const { logs, startPolling, stopPolling, refreshNow } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    expect(logs.value).toHaveLength(1)
    stopPolling()
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000 }),
      makeLog({ createdAt: 1000 })
    ])
    await refreshNow()
    expect(logs.value).toHaveLength(2)
    // refreshNow 后游标清零，应以无参调用重建
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenLastCalledWith()
  })

  it('不同数据源的同指纹记录不互相误判', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000 })
    ])
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce([
      makeLog({ createdAt: 2000, dbType: 'CCS', providerId: 'openai' })
    ])
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenCalledTimes(2)
    expect(logs.value).toHaveLength(2)
    stopPolling()
  })
})
