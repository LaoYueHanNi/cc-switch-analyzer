import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useQueryStore } from '@/stores/query'
import { platformAdapter } from '@/platform'
import type { FilterParams } from '@/types/database'
import type { PrecomputeQueryResult } from '@/types/common'

vi.mock('@/platform', () => ({
  platformAdapter: {
    queryPrecompute: vi.fn(),
    querySummary: vi.fn(),
    queryByModel: vi.fn(),
    queryByProvider: vi.fn()
  }
}))

function makeParams(providerId: string): FilterParams {
  return { fromDate: null, toDate: null, providerId, modelId: '' }
}

function makeResult(totalRequests: number): PrecomputeQueryResult {
  return {
    summary: {
      totalRequests,
      successCount: totalRequests,
      totalInput: 0,
      totalOutput: 0,
      totalCacheRead: 0,
      totalCacheCreation: 0,
      avgLatency: 0
    },
    modelBreakdown: [],
    providerBreakdown: [],
    precomputed: {
      modelCosts: {},
      modelCostBreakdown: {},
      providerCosts: {},
      dayCostMap: {},
      dayRequestsMap: {},
      dayInputTokens: {},
      dayOutputTokens: {},
      dayCacheRead: {},
      dayCacheCreation: {},
      dayLatencySum: {},
      dayLatencyCount: {},
      dailyByModel: {}
    }
  }
}

// ---------------------------------------------------------------------------
// executeQuery 竞态防护：自增请求 ID 丢弃过期响应
// ---------------------------------------------------------------------------
describe('useQueryStore.executeQuery 竞态防护', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('先发请求晚返回时不应覆盖后发请求（晚返回被丢弃）', async () => {
    const store = useQueryStore()
    let resolveFirst: (v: PrecomputeQueryResult) => void = () => {}
    const firstPromise = new Promise<PrecomputeQueryResult>(resolve => { resolveFirst = resolve })

    vi.mocked(platformAdapter.queryPrecompute)
      .mockImplementationOnce(() => firstPromise)
      .mockImplementationOnce(() => Promise.resolve(makeResult(2)))

    const p1 = store.executeQuery(makeParams('provider-a'), true)
    const p2 = store.executeQuery(makeParams('provider-b'), true)

    // 后发请求先返回，应正常生效
    await p2
    expect(store.summary?.totalRequests).toBe(2)

    // 先发请求这时才返回，属于过期响应，应被丢弃
    resolveFirst(makeResult(1))
    await p1
    expect(store.summary?.totalRequests).toBe(2)
  })

  it('过期请求 catch 分支也应被丢弃，不覆盖已生效的结果', async () => {
    const store = useQueryStore()
    let rejectFirst: (err: unknown) => void = () => {}
    const firstPromise = new Promise<PrecomputeQueryResult>((_, reject) => { rejectFirst = reject })

    vi.mocked(platformAdapter.queryPrecompute)
      .mockImplementationOnce(() => firstPromise)
      .mockImplementationOnce(() => Promise.resolve(makeResult(9)))
    vi.mocked(platformAdapter.querySummary).mockResolvedValue(makeResult(1).summary)
    vi.mocked(platformAdapter.queryByModel).mockResolvedValue([])
    vi.mocked(platformAdapter.queryByProvider).mockResolvedValue([])

    const p1 = store.executeQuery(makeParams('provider-a'), true)
    const p2 = store.executeQuery(makeParams('provider-b'), true)

    await p2
    expect(store.summary?.totalRequests).toBe(9)

    rejectFirst(new Error('boom'))
    await p1
    expect(store.summary?.totalRequests).toBe(9)
  })

  it('loading 应在最新请求完成后才置为 false', async () => {
    const store = useQueryStore()
    let resolveFirst: (v: PrecomputeQueryResult) => void = () => {}
    const firstPromise = new Promise<PrecomputeQueryResult>(resolve => { resolveFirst = resolve })

    vi.mocked(platformAdapter.queryPrecompute)
      .mockImplementationOnce(() => firstPromise)
      .mockImplementationOnce(() => Promise.resolve(makeResult(2)))

    const p1 = store.executeQuery(makeParams('provider-a'), true)
    const p2 = store.executeQuery(makeParams('provider-b'), true)
    await p2
    expect(store.loading).toBe(false)

    resolveFirst(makeResult(1))
    await p1
    // 过期请求的 finally 不应再次触碰 loading
    expect(store.loading).toBe(false)
    expect(store.summary?.totalRequests).toBe(2)
  })
})
