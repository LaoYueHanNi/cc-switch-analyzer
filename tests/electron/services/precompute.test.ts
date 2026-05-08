import { describe, it, expect } from 'vitest'
import { precomputeCosts, computeSessionCosts, computeSessionModelCosts } from '../../../electron/main/services/precompute'
import { PricingEngine } from '../../../electron/main/services/pricing-engine'
import { makeAppDbMock, makeModelPricingRow } from '../fixtures/pricing-mocks'
import type { TokenDimensions } from '../../../electron/main/services/pricing-engine'
import type { DailyTrendRow, ProviderModelToken, SessionModelToken, SessionRequestToken } from '../../../electron/main/services/external-db'

// ═══════════════════════════════════════════════════════════════════
// 常量
// ═══════════════════════════════════════════════════════════════════
const MODEL_SONNET = 'claude-sonnet-4'
const MODEL_HAIKU = 'claude-haiku-4'

// ═══════════════════════════════════════════════════════════════════
// 辅助：创建带云端基础定价的 PricingEngine
// ═══════════════════════════════════════════════════════════════════
function createEngine() {
  const mock = makeAppDbMock({
    cloudBase: [
      makeModelPricingRow({ modelId: MODEL_SONNET, inputCostPerMillion: 21, outputCostPerMillion: 105, cacheReadCostPerMillion: 2.1, cacheCreationCostPerMillion: 26.25 }),
      makeModelPricingRow({ modelId: MODEL_HAIKU, inputCostPerMillion: 4.2, outputCostPerMillion: 21, cacheReadCostPerMillion: 0.42, cacheCreationCostPerMillion: 5.25 }),
    ]
  })
  const engine = new PricingEngine(mock as any)
  engine.refresh()
  return engine
}

// 辅助：计算预期费用
function expectedCost(
  input: number, output: number, cacheRead: number, cacheCreation: number,
  inputCost: number, outputCost: number, cacheReadCost: number, cacheCreationCost: number
): number {
  return (input * inputCost + output * outputCost + cacheRead * cacheReadCost + cacheCreation * cacheCreationCost) / 1_000_000
}

// ═══════════════════════════════════════════════════════════════════
// precomputeCosts
// ═══════════════════════════════════════════════════════════════════
describe('precomputeCosts', () => {
  // ─── 1. 空输入 ───
  it('空输入 → 所有 map 为空', () => {
    const engine = createEngine()
    const result = precomputeCosts([], [], engine)

    expect(result.modelCosts).toEqual({})
    expect(result.modelCostBreakdown).toEqual({})
    expect(result.providerCosts).toEqual({})
    expect(result.dayCostMap).toEqual({})
    expect(result.dayRequestsMap).toEqual({})
    expect(result.dayInputTokens).toEqual({})
    expect(result.dayOutputTokens).toEqual({})
    expect(result.dayLatencySum).toEqual({})
    expect(result.dayLatencyCount).toEqual({})
    expect(result.dailyByModel).toEqual({})
  })

  // ─── 2. 单行数据 ───
  it('单行数据 → 正确计算 dayCostMap / modelCosts / dayRequestsMap', () => {
    const engine = createEngine()
    const row: DailyTrendRow = {
      day: '2024-01-15',
      model: MODEL_SONNET,
      requests: 10,
      inputTokens: 1_000_000,
      outputTokens: 500_000,
      cacheRead: 200_000,
      cacheCreation: 100_000,
      avgLatency: 1500,
    }

    const result = precomputeCosts([row], [], engine)

    // 预期费用 = (1M*21 + 500K*105 + 200K*2.1 + 100K*26.25) / 1M = 76.545
    const cost = expectedCost(1_000_000, 500_000, 200_000, 100_000, 21, 105, 2.1, 26.25)
    expect(result.dayCostMap['2024-01-15']).toBeCloseTo(cost, 6)
    expect(result.modelCosts[MODEL_SONNET]).toBeCloseTo(cost, 6)
    expect(result.dayRequestsMap['2024-01-15']).toBe(10)
  })

  // ─── 3. 多天单模型 ───
  it('多天单模型 → 模型费用累加，dayCostMap 各自独立', () => {
    const engine = createEngine()
    const rows: DailyTrendRow[] = [
      {
        day: '2024-01-15',
        model: MODEL_SONNET,
        requests: 10,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
        avgLatency: 1500,
      },
      {
        day: '2024-01-16',
        model: MODEL_SONNET,
        requests: 5,
        inputTokens: 500_000,
        outputTokens: 250_000,
        cacheRead: 100_000,
        cacheCreation: 50_000,
        avgLatency: 2000,
      },
    ]

    const result = precomputeCosts(rows, [], engine)

    const cost1 = expectedCost(1_000_000, 500_000, 200_000, 100_000, 21, 105, 2.1, 26.25)
    const cost2 = expectedCost(500_000, 250_000, 100_000, 50_000, 21, 105, 2.1, 26.25)

    // 模型费用应累加
    expect(result.modelCosts[MODEL_SONNET]).toBeCloseTo(cost1 + cost2, 6)
    // 每天费用各自独立
    expect(result.dayCostMap['2024-01-15']).toBeCloseTo(cost1, 6)
    expect(result.dayCostMap['2024-01-16']).toBeCloseTo(cost2, 6)
  })

  // ─── 4. 供应商按 Token 比例分配 ───
  it('供应商按 Token 比例分配费用', () => {
    const engine = createEngine()
    const rows: DailyTrendRow[] = [
      {
        day: '2024-01-15',
        model: MODEL_SONNET,
        requests: 15,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
        avgLatency: 1500,
      },
    ]

    // Provider A: 1080000 tokens, Provider B: 720000 tokens, 总计 1800000
    const providerTokens: ProviderModelToken[] = [
      { providerId: 'provider-a', model: MODEL_SONNET, inputTokens: 600_000, outputTokens: 300_000, cacheRead: 120_000, cacheCreation: 60_000 },
      { providerId: 'provider-b', model: MODEL_SONNET, inputTokens: 400_000, outputTokens: 200_000, cacheRead: 80_000, cacheCreation: 40_000 },
    ]

    const result = precomputeCosts(rows, providerTokens, engine)

    const totalModelCost = expectedCost(1_000_000, 500_000, 200_000, 100_000, 21, 105, 2.1, 26.25)
    expect(result.providerCosts['provider-a']).toBeCloseTo(totalModelCost * 0.6, 6)
    expect(result.providerCosts['provider-b']).toBeCloseTo(totalModelCost * 0.4, 6)
  })

  // ─── 5. 无定价数据模型 ───
  it('无定价数据模型 → dayCost 为 0，不崩溃', () => {
    const engine = createEngine()
    const row: DailyTrendRow = {
      day: '2024-01-15',
      model: 'unknown-model',
      requests: 5,
      inputTokens: 100_000,
      outputTokens: 50_000,
      cacheRead: 10_000,
      cacheCreation: 5_000,
      avgLatency: 1000,
    }

    const result = precomputeCosts([row], [], engine)

    expect(result.dayCostMap['2024-01-15']).toBe(0)
    expect(result.modelCosts['unknown-model']).toBe(0)
    // 请求数和延迟仍应记录
    expect(result.dayRequestsMap['2024-01-15']).toBe(5)
    expect(result.dayLatencySum['2024-01-15']).toBe(5000)
    expect(result.dayLatencyCount['2024-01-15']).toBe(5)
  })

  // ─── 6. dailyByModel 分组 ───
  it('同一天两个模型 → dailyByModel 各自分组', () => {
    const engine = createEngine()
    const rows: DailyTrendRow[] = [
      {
        day: '2024-01-15',
        model: MODEL_SONNET,
        requests: 10,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
        avgLatency: 1500,
      },
      {
        day: '2024-01-15',
        model: MODEL_HAIKU,
        requests: 20,
        inputTokens: 500_000,
        outputTokens: 250_000,
        cacheRead: 50_000,
        cacheCreation: 25_000,
        avgLatency: 800,
      },
    ]

    const result = precomputeCosts(rows, [], engine)

    expect(result.dailyByModel[MODEL_SONNET]).toHaveLength(1)
    expect(result.dailyByModel[MODEL_HAIKU]).toHaveLength(1)
    expect(result.dailyByModel[MODEL_SONNET][0].model).toBe(MODEL_SONNET)
    expect(result.dailyByModel[MODEL_HAIKU][0].model).toBe(MODEL_HAIKU)

    // 当天费用应为两个模型的费用之和
    const sonnetCost = expectedCost(1_000_000, 500_000, 200_000, 100_000, 21, 105, 2.1, 26.25)
    const haikuCost = expectedCost(500_000, 250_000, 50_000, 25_000, 4.2, 21, 0.42, 5.25)
    expect(result.dayCostMap['2024-01-15']).toBeCloseTo(sonnetCost + haikuCost, 6)
  })

  // ─── 7. 延迟统计累积 ───
  it('延迟统计累积 → dayLatencySum = avgLatency × requests，dayLatencyCount = requests', () => {
    const engine = createEngine()
    const rows: DailyTrendRow[] = [
      {
        day: '2024-01-15',
        model: MODEL_SONNET,
        requests: 10,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
        avgLatency: 1500,
      },
      {
        day: '2024-01-15',
        model: MODEL_HAIKU,
        requests: 5,
        inputTokens: 500_000,
        outputTokens: 250_000,
        cacheRead: 50_000,
        cacheCreation: 25_000,
        avgLatency: 800,
      },
    ]

    const result = precomputeCosts(rows, [], engine)

    // 10 * 1500 + 5 * 800 = 15000 + 4000 = 19000
    expect(result.dayLatencySum['2024-01-15']).toBe(19000)
    expect(result.dayLatencyCount['2024-01-15']).toBe(15)
  })

  // ─── 8. modelCostBreakdown 分解 ───
  it('modelCostBreakdown 返回四维费用分解', () => {
    const engine = createEngine()
    const row: DailyTrendRow = {
      day: '2024-01-15',
      model: MODEL_SONNET,
      requests: 10,
      inputTokens: 1_000_000,
      outputTokens: 500_000,
      cacheRead: 200_000,
      cacheCreation: 100_000,
      avgLatency: 1500,
    }

    const result = precomputeCosts([row], [], engine)

    const bd = result.modelCostBreakdown[MODEL_SONNET]
    expect(bd).toBeDefined()
    expect(bd![0]).toBeCloseTo(1_000_000 * 21 / 1_000_000, 10)       // input: 21
    expect(bd![1]).toBeCloseTo(500_000 * 105 / 1_000_000, 10)         // output: 52.5
    expect(bd![2]).toBeCloseTo(200_000 * 2.1 / 1_000_000, 10)         // cacheRead: 0.42
    expect(bd![3]).toBeCloseTo(100_000 * 26.25 / 1_000_000, 10)       // cacheCreation: 2.625
  })

  // ─── 9. dayInputTokens / dayOutputTokens 累积 ───
  it('dayInputTokens / dayOutputTokens 按天累加', () => {
    const engine = createEngine()
    const rows: DailyTrendRow[] = [
      {
        day: '2024-01-15',
        model: MODEL_SONNET,
        requests: 10,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
        avgLatency: 1500,
      },
      {
        day: '2024-01-15',
        model: MODEL_HAIKU,
        requests: 5,
        inputTokens: 300_000,
        outputTokens: 150_000,
        cacheRead: 50_000,
        cacheCreation: 25_000,
        avgLatency: 800,
      },
    ]

    const result = precomputeCosts(rows, [], engine)

    expect(result.dayInputTokens['2024-01-15']).toBe(1_300_000)
    expect(result.dayOutputTokens['2024-01-15']).toBe(650_000)
  })

  // ─── 10. 供应商 Token 全为 0 → 不崩溃 ───
  it('供应商 Token 全为 0 → providerCosts 为空，不崩溃', () => {
    const engine = createEngine()
    const row: DailyTrendRow = {
      day: '2024-01-15',
      model: MODEL_SONNET,
      requests: 5,
      inputTokens: 100_000,
      outputTokens: 50_000,
      cacheRead: 10_000,
      cacheCreation: 5_000,
      avgLatency: 1000,
    }
    const providerTokens: ProviderModelToken[] = [
      { providerId: 'provider-a', model: MODEL_SONNET, inputTokens: 0, outputTokens: 0, cacheRead: 0, cacheCreation: 0 },
    ]

    const result = precomputeCosts([row], providerTokens, engine)

    expect(result.providerCosts['provider-a']).toBeUndefined()
  })
})

// ═══════════════════════════════════════════════════════════════════
// computeSessionCosts
// ═══════════════════════════════════════════════════════════════════
describe('computeSessionCosts', () => {
  // ─── 1. 空输入 ───
  it('空输入 → 返回空对象', () => {
    const engine = createEngine()
    const result = computeSessionCosts([], engine)
    expect(result).toEqual({})
  })

  // ─── 2. 单请求 ───
  it('单请求 → 正确计算 session 费用', () => {
    const engine = createEngine()
    // "2024-01-15T00:00:00Z" → epoch 1705276800
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        createdAt: 1705276800,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
      },
    ]

    const result = computeSessionCosts(requests, engine)

    const cost = expectedCost(1_000_000, 500_000, 200_000, 100_000, 21, 105, 2.1, 26.25)
    expect(result['sess-1']).toBeCloseTo(cost, 6)
  })

  // ─── 3. 同 session 多请求累加 ───
  it('同 session 多请求 → 费用累加', () => {
    const engine = createEngine()
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        createdAt: 1705276800,
        inputTokens: 500_000,
        outputTokens: 250_000,
        cacheRead: 100_000,
        cacheCreation: 50_000,
      },
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        createdAt: 1705280400,
        inputTokens: 300_000,
        outputTokens: 150_000,
        cacheRead: 60_000,
        cacheCreation: 30_000,
      },
    ]

    const result = computeSessionCosts(requests, engine)

    const cost1 = expectedCost(500_000, 250_000, 100_000, 50_000, 21, 105, 2.1, 26.25)
    const cost2 = expectedCost(300_000, 150_000, 60_000, 30_000, 21, 105, 2.1, 26.25)
    expect(result['sess-1']).toBeCloseTo(cost1 + cost2, 6)
  })

  // ─── 4. 无定价模型跳过 ───
  it('无定价模型 → session 不在结果中', () => {
    const engine = createEngine()
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-unknown',
        model: 'unknown-model',
        createdAt: 1705276800,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
      },
    ]

    const result = computeSessionCosts(requests, engine)

    expect(result['sess-unknown']).toBeUndefined()
  })
})

// ═══════════════════════════════════════════════════════════════════
// computeSessionModelCosts
// ═══════════════════════════════════════════════════════════════════
describe('computeSessionModelCosts', () => {
  // ─── 1. 空输入 ───
  it('空输入 → 返回空对象', () => {
    const engine = createEngine()
    const result = computeSessionModelCosts([], [], engine)
    expect(result).toEqual({})
  })

  // ─── 2. 单 session 单模型 ───
  it('单 session 单模型 → 验证 cost / breakdown / tokens', () => {
    const engine = createEngine()
    const sessionModelTokens: SessionModelToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
      },
    ]
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        createdAt: 1705276800,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
      },
    ]

    const result = computeSessionModelCosts(requests, sessionModelTokens, engine)

    expect(result['sess-1']).toBeDefined()
    expect(result['sess-1'][MODEL_SONNET]).toBeDefined()

    const entry = result['sess-1'][MODEL_SONNET]
    const cost = expectedCost(1_000_000, 500_000, 200_000, 100_000, 21, 105, 2.1, 26.25)
    expect(entry.cost).toBeCloseTo(cost, 6)

    // breakdown 各分量
    expect(entry.breakdown[0]).toBeCloseTo(1_000_000 * 21 / 1_000_000, 10)
    expect(entry.breakdown[1]).toBeCloseTo(500_000 * 105 / 1_000_000, 10)
    expect(entry.breakdown[2]).toBeCloseTo(200_000 * 2.1 / 1_000_000, 10)
    expect(entry.breakdown[3]).toBeCloseTo(100_000 * 26.25 / 1_000_000, 10)

    // tokens 来自 sessionModelTokens
    expect(entry.tokens.input).toBe(1_000_000)
    expect(entry.tokens.output).toBe(500_000)
    expect(entry.tokens.cacheRead).toBe(200_000)
    expect(entry.tokens.cacheCreation).toBe(100_000)
  })

  // ─── 3. 单 session 多模型 ───
  it('单 session 多模型 → 各模型独立记录', () => {
    const engine = createEngine()
    const sessionModelTokens: SessionModelToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        inputTokens: 800_000,
        outputTokens: 400_000,
        cacheRead: 100_000,
        cacheCreation: 50_000,
      },
      {
        sessionId: 'sess-1',
        model: MODEL_HAIKU,
        inputTokens: 200_000,
        outputTokens: 100_000,
        cacheRead: 20_000,
        cacheCreation: 10_000,
      },
    ]
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        createdAt: 1705276800,
        inputTokens: 800_000,
        outputTokens: 400_000,
        cacheRead: 100_000,
        cacheCreation: 50_000,
      },
      {
        sessionId: 'sess-1',
        model: MODEL_HAIKU,
        createdAt: 1705280400,
        inputTokens: 200_000,
        outputTokens: 100_000,
        cacheRead: 20_000,
        cacheCreation: 10_000,
      },
    ]

    const result = computeSessionModelCosts(requests, sessionModelTokens, engine)

    expect(Object.keys(result['sess-1'])).toHaveLength(2)

    const sonnetCost = expectedCost(800_000, 400_000, 100_000, 50_000, 21, 105, 2.1, 26.25)
    const haikuCost = expectedCost(200_000, 100_000, 20_000, 10_000, 4.2, 21, 0.42, 5.25)

    expect(result['sess-1'][MODEL_SONNET].cost).toBeCloseTo(sonnetCost, 6)
    expect(result['sess-1'][MODEL_HAIKU].cost).toBeCloseTo(haikuCost, 6)
  })

  // ─── 4. tokens 字段来源 ───
  it('tokens 字段来源为 sessionModelTokens，非请求级重新计算', () => {
    const engine = createEngine()
    // sessionModelTokens 中的 Token 量与请求不同
    const sessionModelTokens: SessionModelToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        inputTokens: 2_000_000,
        outputTokens: 1_000_000,
        cacheRead: 500_000,
        cacheCreation: 200_000,
      },
    ]
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        createdAt: 1705276800,
        inputTokens: 500_000,
        outputTokens: 250_000,
        cacheRead: 100_000,
        cacheCreation: 50_000,
      },
    ]

    const result = computeSessionModelCosts(requests, sessionModelTokens, engine)

    const entry = result['sess-1'][MODEL_SONNET]
    // tokens 来自 sessionModelTokens
    expect(entry.tokens.input).toBe(2_000_000)
    expect(entry.tokens.output).toBe(1_000_000)
    expect(entry.tokens.cacheRead).toBe(500_000)
    expect(entry.tokens.cacheCreation).toBe(200_000)

    // cost 来自请求级累加，而非 sessionModelTokens 的量
    const reqCost = expectedCost(500_000, 250_000, 100_000, 50_000, 21, 105, 2.1, 26.25)
    expect(entry.cost).toBeCloseTo(reqCost, 6)
  })

  // ─── 5. session/model 不在映射中 → 跳过 ───
  it('请求的 session+model 不在 sessionModelTokens 中 → 跳过', () => {
    const engine = createEngine()
    // sessionModelTokens 中没有 sess-missing
    const sessionModelTokens: SessionModelToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
      },
    ]
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-missing',
        model: MODEL_SONNET,
        createdAt: 1705276800,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
      },
      {
        sessionId: 'sess-1',
        model: MODEL_HAIKU,  // sess-1 中没有 haiku 模型
        createdAt: 1705276800,
        inputTokens: 100_000,
        outputTokens: 50_000,
        cacheRead: 10_000,
        cacheCreation: 5_000,
      },
    ]

    const result = computeSessionModelCosts(requests, sessionModelTokens, engine)

    // sess-missing 不在结果中
    expect(result['sess-missing']).toBeUndefined()
    // sess-1 只有 sonnet，haiku 的请求被跳过
    expect(Object.keys(result['sess-1'])).toHaveLength(1)
    expect(result['sess-1'][MODEL_HAIKU]).toBeUndefined()
    // sess-1 的 sonnet cost = 0（没有匹配的请求）
    expect(result['sess-1'][MODEL_SONNET].cost).toBe(0)
  })

  // ─── 6. 多请求累加到同一 session+model ───
  it('同一 session+model 的多请求 → 费用和分解都累加', () => {
    const engine = createEngine()
    const sessionModelTokens: SessionModelToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        inputTokens: 1_500_000,
        outputTokens: 750_000,
        cacheRead: 300_000,
        cacheCreation: 150_000,
      },
    ]
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        createdAt: 1705276800,
        inputTokens: 1_000_000,
        outputTokens: 500_000,
        cacheRead: 200_000,
        cacheCreation: 100_000,
      },
      {
        sessionId: 'sess-1',
        model: MODEL_SONNET,
        createdAt: 1705280400,
        inputTokens: 500_000,
        outputTokens: 250_000,
        cacheRead: 100_000,
        cacheCreation: 50_000,
      },
    ]

    const result = computeSessionModelCosts(requests, sessionModelTokens, engine)

    const cost1 = expectedCost(1_000_000, 500_000, 200_000, 100_000, 21, 105, 2.1, 26.25)
    const cost2 = expectedCost(500_000, 250_000, 100_000, 50_000, 21, 105, 2.1, 26.25)

    const entry = result['sess-1'][MODEL_SONNET]
    expect(entry.cost).toBeCloseTo(cost1 + cost2, 6)

    // breakdown 也应累加
    const bd1Input = 1_000_000 * 21 / 1_000_000
    const bd2Input = 500_000 * 21 / 1_000_000
    expect(entry.breakdown[0]).toBeCloseTo(bd1Input + bd2Input, 10)
  })

  // ─── 7. 无定价模型请求 → cost 保持 0 ───
  it('请求为无定价模型 → 该 session+model cost 为 0（不崩溃）', () => {
    const engine = createEngine()
    const sessionModelTokens: SessionModelToken[] = [
      {
        sessionId: 'sess-1',
        model: 'unknown-model',
        inputTokens: 500_000,
        outputTokens: 250_000,
        cacheRead: 50_000,
        cacheCreation: 25_000,
      },
    ]
    const requests: SessionRequestToken[] = [
      {
        sessionId: 'sess-1',
        model: 'unknown-model',
        createdAt: 1705276800,
        inputTokens: 500_000,
        outputTokens: 250_000,
        cacheRead: 50_000,
        cacheCreation: 25_000,
      },
    ]

    const result = computeSessionModelCosts(requests, sessionModelTokens, engine)

    // session+model 存在于 sessionModelTokens，但无定价
    expect(result['sess-1']['unknown-model']).toBeDefined()
    expect(result['sess-1']['unknown-model'].cost).toBe(0)
    expect(result['sess-1']['unknown-model'].breakdown).toEqual([0, 0, 0, 0])
    // tokens 仍来自 sessionModelTokens
    expect(result['sess-1']['unknown-model'].tokens.input).toBe(500_000)
  })
})
