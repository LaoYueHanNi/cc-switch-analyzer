import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { useRealtimePolling } from '@/composables/useRealtimePolling'
import { platformAdapter } from '@/platform'
import type { RealtimeRequestLog, RealtimeLogsPage } from '@/types/database'

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

// 工厂：模拟后端 RealtimeLogsPage 响应
function page(logs: RealtimeRequestLog[], absorbed: string[] = []): RealtimeLogsPage {
  return { logs, absorbed }
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
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 }),
      makeLog({ createdAt: 1000 })
    ]))
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    expect(logs.value).toHaveLength(2)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenCalledWith() // 无参首屏
    stopPolling()
  })

  it('增量轮询重复返回已见记录时不追加（>= 游标语义兜底）', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 })
    ]))
    // 下一轮：>= 游标语义重复返回同一条（真实记录 createdAt 为截断秒、毫秒尾数不参与比较）
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 })
    ]))
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
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000, outputTokens: 10 })
    ]))
    // 同一秒内后到的记录（createdAt 同为 2000）+ 重复的已见记录
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000, outputTokens: 99 }),
      makeLog({ createdAt: 2000, outputTokens: 10 })
    ]))
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenCalledTimes(2)
    expect(logs.value).toHaveLength(2)
    expect(logs.value[0].outputTokens).toBe(99) // 新记录在前
    stopPolling()
  })

  it.each([
    ['原始 ID 补全为真实会话', '0a9c789d-9ae3-4532-8a6c-a3ed77306382', 'codex-session'],
    ['缓存重建后暂时失去映射', 'codex-session', '0a9c789d-9ae3-4532-8a6c-a3ed77306382'],
  ])('Codex %s时不重复追加请求或累计用量', async (_, firstSessionId, nextSessionId) => {
    const request = makeLog({
      createdAt: 1792056248,
      dbType: 'CCS',
      providerId: 'openai',
      sourceSessionId: '0a9c789d-9ae3-4532-8a6c-a3ed77306382',
      inputTokens: 7029,
      outputTokens: 6692,
      cacheReadTokens: 71808,
      latencyMs: 78455,
      totalCost: 0.42,
    })
    vi.mocked(platformAdapter.queryRealtimeLogs)
      .mockResolvedValueOnce(page([{ ...request, sessionId: firstSessionId }]))
      .mockResolvedValueOnce(page([{ ...request, sessionId: nextSessionId }]))

    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    try {
      await vi.advanceTimersByTimeAsync(0)
      await vi.advanceTimersByTimeAsync(10_000)
      expect(platformAdapter.queryRealtimeLogs).toHaveBeenLastCalledWith(request.createdAt)
      expect(logs.value).toHaveLength(1)
      expect(logs.value.reduce((sum, row) => sum + row.inputTokens + row.outputTokens + row.cacheReadTokens + row.cacheCreationTokens, 0)).toBe(85529)
      expect(logs.value.reduce((sum, row) => sum + row.totalCost, 0)).toBe(0.42)
    } finally {
      stopPolling()
    }
  })

  it('原始 ID 不同的同秒同用量请求映射到同一 Codex 会话时仍分别保留', async () => {
    const request = makeLog({
      createdAt: 2000,
      dbType: 'CCS',
      providerId: 'openai',
      sessionId: 'codex-session',
      sourceSessionId: 'source-request-a',
    })
    vi.mocked(platformAdapter.queryRealtimeLogs)
      .mockResolvedValueOnce(page([request]))
      .mockResolvedValueOnce(page([
        { ...request, sourceSessionId: 'source-request-b' },
        request,
      ]))

    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    try {
      await vi.advanceTimersByTimeAsync(0)
      await vi.advanceTimersByTimeAsync(10_000)
      expect(logs.value.map(row => row.sourceSessionId)).toEqual(['source-request-b', 'source-request-a'])
    } finally {
      stopPolling()
    }
  })

  it('新秒记录追加且游标推进', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 })
    ]))
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 3000 })
    ]))
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(logs.value).toHaveLength(2)
    // 下一轮请求携带推进后的游标 3000
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([]))
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenLastCalledWith(3000)
    stopPolling()
  })

  it('增量全为已见记录时游标不回退、列表不变', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 })
    ]))
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 })
    ]))
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    // 第三轮游标仍应为 2000（无新记录时不推进）
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([]))
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenLastCalledWith(2000)
    expect(logs.value).toHaveLength(1)
    stopPolling()
  })

  it('refreshNow 清空游标与指纹集重新全量拉取', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 })
    ]))
    const { logs, startPolling, stopPolling, refreshNow } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    expect(logs.value).toHaveLength(1)
    stopPolling()
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 }),
      makeLog({ createdAt: 1000 })
    ]))
    await refreshNow()
    expect(logs.value).toHaveLength(2)
    // refreshNow 后游标清零，应以无参调用重建
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenLastCalledWith()
  })

  it('不同数据源的同指纹记录不互相误判', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000 })
    ]))
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000, dbType: 'CCS', providerId: 'openai' })
    ]))
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(platformAdapter.queryRealtimeLogs).toHaveBeenCalledTimes(2)
    expect(logs.value).toHaveLength(2)
    stopPolling()
  })

  it('吸收清单移除早前轮次残留的网关行', async () => {
    // 第一轮：网关行先于端侧行被单独放出一轮（两源扫描轮错峰）
    const gatewayRow = makeLog({
      createdAt: 1000, dbType: 'Magpie', providerId: 'Magpie',
      requestId: 'magpie-hash-1'
    })
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([gatewayRow]))
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    expect(logs.value).toHaveLength(1)

    // 第二轮：端侧行到达，配对成功只返回端侧；absorbed 携带被吸收网关行 ID
    const endpointRow = makeLog({
      createdAt: 1017, dbType: 'DSH', providerId: 'DSH',
      requestId: 'dsh-msg-1'
    })
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(
      page([endpointRow], ['magpie-hash-1'])
    )
    await vi.advanceTimersByTimeAsync(10_000)
    // 残留的 Magpie 行被移除，只剩端侧行
    expect(logs.value).toHaveLength(1)
    expect(logs.value[0].dbType).toBe('DSH')
    expect(logs.value[0].requestId).toBe('dsh-msg-1')
    stopPolling()
  })

  it('吸收清单中的未知 ID 不影响列表', async () => {
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(page([
      makeLog({ createdAt: 2000, requestId: 'keep-1' })
    ]))
    vi.mocked(platformAdapter.queryRealtimeLogs).mockResolvedValueOnce(
      page([], ['already-evicted-id'])
    )
    const { logs, startPolling, stopPolling } = useRealtimePolling()
    startPolling()
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(logs.value).toHaveLength(1)
    expect(logs.value[0].requestId).toBe('keep-1')
    stopPolling()
  })
})
