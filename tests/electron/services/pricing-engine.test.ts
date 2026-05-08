import { describe, it, expect } from 'vitest'
import { PricingEngine } from '../../../electron/main/services/pricing-engine'
import {
  makeMergedPricing,
  makeContextTier,
  makeAppDbMock,
  makeModelPricingRow,
  makePricingOverride,
  makeTimeRule,
  makeCloudTimeRule,
  makeTokens,
} from '../fixtures/pricing-mocks'

// ═══════════════════════════════════════════════════════════════════
// 常量：默认模型 ID，避免魔术字符串
// ═══════════════════════════════════════════════════════════════════
const MODEL_ID = 'claude-sonnet-4-20250514'

// ═══════════════════════════════════════════════════════════════════
// 1. calculateCost & calculateCostBreakdown
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — calculateCost & calculateCostBreakdown', () => {
  const engine = new PricingEngine(makeAppDbMock())

  it('零 token 用量 → 费用为 0', () => {
    const pricing = makeMergedPricing()
    const tokens = makeTokens({ input: 0, output: 0, cacheRead: 0, cacheCreation: 0 })
    expect(engine.calculateCost(pricing, tokens)).toBe(0)
  })

  it('仅 input 维度：1M tokens × 21元/百万 = 21', () => {
    const pricing = makeMergedPricing({ inputCostPerMillion: 21 })
    const tokens = makeTokens({ input: 1_000_000, output: 0, cacheRead: 0, cacheCreation: 0 })
    expect(engine.calculateCost(pricing, tokens)).toBe(21)
  })

  it('四维完整计算公式正确', () => {
    const pricing = makeMergedPricing({
      inputCostPerMillion: 21,
      outputCostPerMillion: 105,
      cacheReadCostPerMillion: 2.1,
      cacheCreationCostPerMillion: 26.25,
    })
    const tokens = makeTokens({
      input: 1_000_000,
      output: 500_000,
      cacheRead: 200_000,
      cacheCreation: 100_000,
    })

    // 预期: (1M*21 + 500K*105 + 200K*2.1 + 100K*26.25) / 1M
    //     = 21 + 52.5 + 0.42 + 2.625 = 76.545
    const expected = (1_000_000 * 21 + 500_000 * 105 + 200_000 * 2.1 + 100_000 * 26.25) / 1_000_000
    expect(engine.calculateCost(pricing, tokens)).toBeCloseTo(expected, 10)
  })

  it('calculateCostBreakdown 返回四维分解', () => {
    const pricing = makeMergedPricing({
      inputCostPerMillion: 21,
      outputCostPerMillion: 105,
      cacheReadCostPerMillion: 2.1,
      cacheCreationCostPerMillion: 26.25,
    })
    const tokens = makeTokens({
      input: 1_000_000,
      output: 500_000,
      cacheRead: 200_000,
      cacheCreation: 100_000,
    })

    const [inputCost, outputCost, cacheReadCost, cacheCreationCost] =
      engine.calculateCostBreakdown(pricing, tokens)

    expect(inputCost).toBeCloseTo(21, 10)        // 1M * 21 / 1M
    expect(outputCost).toBeCloseTo(52.5, 10)     // 500K * 105 / 1M
    expect(cacheReadCost).toBeCloseTo(0.42, 10)  // 200K * 2.1 / 1M
    expect(cacheCreationCost).toBeCloseTo(2.625, 10) // 100K * 26.25 / 1M
  })

  it('breakdown 各分量之和等于 calculateCost 结果', () => {
    const pricing = makeMergedPricing()
    const tokens = makeTokens()
    const total = engine.calculateCost(pricing, tokens)
    const breakdown = engine.calculateCostBreakdown(pricing, tokens)
    const sum = breakdown.reduce((a, b) => a + b, 0)
    expect(sum).toBeCloseTo(total, 10)
  })
})

// ═══════════════════════════════════════════════════════════════════
// 2. refresh + getPricing（合并逻辑）
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — refresh + getPricing（合并逻辑）', () => {
  it('仅云端基础定价：getPricing 返回 isOverride=false', () => {
    const engine = new PricingEngine(
      makeAppDbMock({ cloudBase: [makeModelPricingRow()] })
    )
    engine.refresh()
    const result = engine.getPricing(MODEL_ID)
    expect(result).not.toBeNull()
    expect(result!.modelId).toBe(MODEL_ID)
    expect(result!.isOverride).toBe(false)
    expect(result!.inputCostPerMillion).toBe(21)
  })

  it('用户覆盖替换云端：getPricing 返回 isOverride=true 且使用覆盖价格', () => {
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        overrides: [makePricingOverride()],
      })
    )
    engine.refresh()
    const result = engine.getPricing(MODEL_ID)
    expect(result).not.toBeNull()
    expect(result!.isOverride).toBe(true)
    expect(result!.inputCostPerMillion).toBe(30) // 覆盖值
    expect(result!.outputCostPerMillion).toBe(150)
  })

  it('无云端基础且无覆盖：getPricing 返回 null', () => {
    const engine = new PricingEngine(makeAppDbMock())
    engine.refresh()
    expect(engine.getPricing(MODEL_ID)).toBeNull()
  })

  it('有云端基础但查询不存在的模型：返回 null', () => {
    const engine = new PricingEngine(
      makeAppDbMock({ cloudBase: [makeModelPricingRow()] })
    )
    engine.refresh()
    expect(engine.getPricing('non-existent-model')).toBeNull()
  })
})

// ═══════════════════════════════════════════════════════════════════
// 3. getPricingAt（时间感知，三级回退）
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — getPricingAt（三级时间回退）', () => {
  // 用户时间规则: [1700000000, 1700086400]
  // 云端时间规则: [1700172800, 1700259200]
  // 云端基础定价始终存在

  function createEngine() {
    const userTimeRule = makeTimeRule({
      startTime: 1700000000,
      endTime: 1700086400,
    })
    const cloudTimeRule = makeCloudTimeRule({
      startTime: 1700172800,
      endTime: 1700259200,
    })
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        timeOverrides: [userTimeRule],
        cloudTimeRules: new Map([[MODEL_ID, [cloudTimeRule]]]),
      })
    )
    engine.refresh()
    return engine
  }

  it('在用户时间范围内 → 返回用户规则定价，isOverride=true', () => {
    const engine = createEngine()
    // 取范围中间的时刻
    const result = engine.getPricingAt(MODEL_ID, 1700043200)
    expect(result).not.toBeNull()
    expect(result!.isOverride).toBe(true)
    expect(result!.inputCostPerMillion).toBe(10.5) // makeTimeRule 默认值
  })

  it('在云端时间范围内（无用户规则匹配）→ 返回云端规则定价，isOverride=false', () => {
    const engine = createEngine()
    const result = engine.getPricingAt(MODEL_ID, 1700172800)
    expect(result).not.toBeNull()
    expect(result!.isOverride).toBe(false)
    expect(result!.inputCostPerMillion).toBe(15) // makeCloudTimeRule 默认值
  })

  it('不在任何时间范围内 → 回退到固定合并定价', () => {
    const engine = createEngine()
    // 1700090000 落在两个范围之间
    const result = engine.getPricingAt(MODEL_ID, 1700090000)
    expect(result).not.toBeNull()
    expect(result!.isOverride).toBe(false)
    expect(result!.inputCostPerMillion).toBe(21) // 云端基础定价
  })

  it('用户时间范围起始边界（startTime）包含在内', () => {
    const engine = createEngine()
    const result = engine.getPricingAt(MODEL_ID, 1700000000)
    expect(result).not.toBeNull()
    expect(result!.isOverride).toBe(true)
    expect(result!.inputCostPerMillion).toBe(10.5)
  })

  it('用户时间范围结束边界（endTime）包含在内', () => {
    const engine = createEngine()
    const result = engine.getPricingAt(MODEL_ID, 1700086400)
    expect(result).not.toBeNull()
    expect(result!.isOverride).toBe(true)
    expect(result!.inputCostPerMillion).toBe(10.5)
  })

  it('云端时间范围起始边界包含在内', () => {
    const engine = createEngine()
    const result = engine.getPricingAt(MODEL_ID, 1700172800)
    expect(result).not.toBeNull()
    expect(result!.isOverride).toBe(false)
    expect(result!.inputCostPerMillion).toBe(15)
  })

  it('模型无任何定价时返回 null', () => {
    const engine = createEngine()
    expect(engine.getPricingAt('non-existent-model', 1700000000)).toBeNull()
  })
})

// ═══════════════════════════════════════════════════════════════════
// 4. getPricingAtWithContext（四级优先级，核心测试）
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — getPricingAtWithContext（四级优先级）', () => {
  // ─── 辅助：创建带上下文档位的用户时间规则 ───
  const EPOCH_T = 1700000000

  function tier(threshold: number, costPerMillion: number): ReturnType<typeof makeContextTier> {
    return makeContextTier({
      threshold,
      inputCostPerMillion: costPerMillion,
      outputCostPerMillion: costPerMillion * 5,
      cacheReadCostPerMillion: costPerMillion * 0.1,
      cacheCreationCostPerMillion: costPerMillion * 1.25,
    })
  }

  // ─── 第 1 级：用户时间规则 + 上下文档位 ───
  describe('第 1 级：用户时间规则 + 上下文档位', () => {
    function createEngine() {
      const userTimeRule = makeTimeRule({
        startTime: EPOCH_T,
        endTime: EPOCH_T + 86400,
        contextTiers: [
          tier(10000, 5),
          tier(50000, 8),
        ],
      })
      const engine = new PricingEngine(
        makeAppDbMock({
          cloudBase: [makeModelPricingRow()],
          timeOverrides: [userTimeRule],
        })
      )
      engine.refresh()
      return engine
    }

    it('contextSize=30000 → 命中档位 threshold=10000', () => {
      const engine = createEngine()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 30000)
      expect(result).not.toBeNull()
      expect(result!.inputCostPerMillion).toBe(5)
      expect(result!.isOverride).toBe(true)
    })

    it('contextSize=60000 → 命中档位 threshold=50000', () => {
      const engine = createEngine()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 60000)
      expect(result).not.toBeNull()
      expect(result!.inputCostPerMillion).toBe(8)
    })

    it('contextSize=5000 → 无档位匹配，使用规则平价', () => {
      const engine = createEngine()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 5000)
      expect(result).not.toBeNull()
      // makeTimeRule 默认 inputCostPerMillion = 10.5
      expect(result!.inputCostPerMillion).toBe(10.5)
    })
  })

  // ─── 第 2 级：云端时间规则 + 上下文档位（无用户规则匹配） ───
  describe('第 2 级：云端时间规则 + 上下文档位', () => {
    function createEngine() {
      const cloudTimeRule = makeCloudTimeRule({
        startTime: EPOCH_T + 172800, // EPOCH_T + 2天
        endTime: EPOCH_T + 259200,   // EPOCH_T + 3天
        contextTiers: [
          tier(20000, 12),
        ],
      })
      const engine = new PricingEngine(
        makeAppDbMock({
          cloudBase: [makeModelPricingRow()],
          cloudTimeRules: new Map([[MODEL_ID, [cloudTimeRule]]]),
        })
      )
      engine.refresh()
      return engine
    }

    it('在云端时间范围内，contextSize 命中档位', () => {
      const engine = createEngine()
      const epoch = EPOCH_T + 200000
      const result = engine.getPricingAtWithContext(MODEL_ID, epoch, 25000)
      expect(result).not.toBeNull()
      expect(result!.inputCostPerMillion).toBe(12)
      // tierToMerged 硬编码 isOverride=true
      expect(result!.isOverride).toBe(true)
    })

    it('在云端时间范围内，contextSize 未命中档位 → 使用规则平价', () => {
      const engine = createEngine()
      const epoch = EPOCH_T + 200000
      const result = engine.getPricingAtWithContext(MODEL_ID, epoch, 5000)
      expect(result).not.toBeNull()
      // makeCloudTimeRule 默认 inputCostPerMillion = 15
      expect(result!.inputCostPerMillion).toBe(15)
      expect(result!.isOverride).toBe(false)
    })
  })

  // ─── 第 3 级：覆盖上下文档位（无时间规则匹配） ───
  describe('第 3 级：覆盖上下文档位', () => {
    function createEngine() {
      const override = makePricingOverride({
        contextTiers: [
          tier(10000, 25),
        ],
      })
      const engine = new PricingEngine(
        makeAppDbMock({
          cloudBase: [makeModelPricingRow()],
          overrides: [override],
        })
      )
      engine.refresh()
      return engine
    }

    it('无时间规则匹配时，命中覆盖档位', () => {
      const engine = createEngine()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T, 15000)
      expect(result).not.toBeNull()
      expect(result!.inputCostPerMillion).toBe(25)
      expect(result!.isOverride).toBe(true)
    })

    it('覆盖档位未命中 → 回退到固定定价', () => {
      const engine = createEngine()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T, 5000)
      expect(result).not.toBeNull()
      // 覆盖价格（makePricingOverride 默认 inputCostPerMillion=30）
      expect(result!.inputCostPerMillion).toBe(30)
    })
  })

  // ─── 第 4 级：固定定价回退 ───
  describe('第 4 级：固定定价回退', () => {
    it('无时间规则、无档位 → 返回固定合并定价', () => {
      const engine = new PricingEngine(
        makeAppDbMock({ cloudBase: [makeModelPricingRow()] })
      )
      engine.refresh()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T, 50000)
      expect(result).not.toBeNull()
      expect(result!.inputCostPerMillion).toBe(21)
      expect(result!.isOverride).toBe(false)
    })

    it('模型不存在时返回 null', () => {
      const engine = new PricingEngine(
        makeAppDbMock({ cloudBase: [makeModelPricingRow()] })
      )
      engine.refresh()
      expect(engine.getPricingAtWithContext('no-model', EPOCH_T, 50000)).toBeNull()
    })
  })

  // ─── 完整优先级链：四级全部存在，用户时间规则档位胜出 ───
  describe('完整优先级链测试', () => {
    function createFullEngine() {
      const userTimeRule = makeTimeRule({
        startTime: EPOCH_T,
        endTime: EPOCH_T + 86400,
        inputCostPerMillion: 10,
        contextTiers: [
          tier(10000, 3), // 用户档位价格
        ],
      })
      const cloudTimeRule = makeCloudTimeRule({
        startTime: EPOCH_T + 172800,
        endTime: EPOCH_T + 259200,
        contextTiers: [
          tier(10000, 12),
        ],
      })
      const override = makePricingOverride({
        contextTiers: [
          tier(10000, 25),
        ],
      })

      const engine = new PricingEngine(
        makeAppDbMock({
          cloudBase: [makeModelPricingRow()],
          timeOverrides: [userTimeRule],
          cloudTimeRules: new Map([[MODEL_ID, [cloudTimeRule]]]),
          overrides: [override],
        })
      )
      engine.refresh()
      return engine
    }

    it('四级全部存在时，用户时间规则档位优先级最高', () => {
      const engine = createFullEngine()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 15000)
      expect(result).not.toBeNull()
      expect(result!.inputCostPerMillion).toBe(3) // 用户时间规则档位
      expect(result!.isOverride).toBe(true)
    })

    it('用户时间范围外，云端时间范围内 → 云端规则档位', () => {
      const engine = createFullEngine()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 200000, 15000)
      expect(result).not.toBeNull()
      expect(result!.inputCostPerMillion).toBe(12) // 云端时间规则档位
      // tierToMerged 硬编码 isOverride=true，但价格来自云端
      expect(result!.isOverride).toBe(true)
    })

    it('所有时间范围外，命中覆盖档位', () => {
      const engine = createFullEngine()
      // EPOCH_T + 500000 已超出所有时间范围
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 500000, 15000)
      expect(result).not.toBeNull()
      expect(result!.inputCostPerMillion).toBe(25) // 覆盖档位
    })

    it('所有时间范围外，覆盖档位未命中 → 固定覆盖定价', () => {
      const engine = createFullEngine()
      const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 500000, 5000)
      expect(result).not.toBeNull()
      // makePricingOverride 默认 inputCostPerMillion=30
      expect(result!.inputCostPerMillion).toBe(30)
    })
  })
})

// ═══════════════════════════════════════════════════════════════════
// 5. getMatchedTierThreshold
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — getMatchedTierThreshold', () => {
  const EPOCH_T = 1700000000

  function createEngineWithContextTiers() {
    const userTimeRule = makeTimeRule({
      startTime: EPOCH_T,
      endTime: EPOCH_T + 86400,
      contextTiers: [
        makeContextTier({ threshold: 10000 }),
        makeContextTier({ threshold: 50000 }),
      ],
    })
    const override = makePricingOverride({
      contextTiers: [
        makeContextTier({ threshold: 20000 }),
      ],
    })
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        timeOverrides: [userTimeRule],
        overrides: [override],
      })
    )
    engine.refresh()
    return engine
  }

  it('用户时间范围内，命中档位 → 返回 threshold 值', () => {
    const engine = createEngineWithContextTiers()
    expect(engine.getMatchedTierThreshold(MODEL_ID, EPOCH_T + 1000, 30000)).toBe(10000)
  })

  it('用户时间范围内，命中最高档位', () => {
    const engine = createEngineWithContextTiers()
    expect(engine.getMatchedTierThreshold(MODEL_ID, EPOCH_T + 1000, 60000)).toBe(50000)
  })

  it('用户时间范围内，contextSize 太小无匹配 → 返回 null', () => {
    const engine = createEngineWithContextTiers()
    expect(engine.getMatchedTierThreshold(MODEL_ID, EPOCH_T + 1000, 5000)).toBeNull()
  })

  it('时间范围外，回退到覆盖档位 → 返回覆盖档位 threshold', () => {
    const engine = createEngineWithContextTiers()
    expect(engine.getMatchedTierThreshold(MODEL_ID, EPOCH_T + 200000, 30000)).toBe(20000)
  })

  it('无任何匹配时返回 null', () => {
    const engine = createEngineWithContextTiers()
    expect(engine.getMatchedTierThreshold('non-existent', EPOCH_T, 30000)).toBeNull()
  })
})

// ═══════════════════════════════════════════════════════════════════
// 6. hasTimePricing
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — hasTimePricing', () => {
  it('无时间规则 → false', () => {
    const engine = new PricingEngine(
      makeAppDbMock({ cloudBase: [makeModelPricingRow()] })
    )
    engine.refresh()
    expect(engine.hasTimePricing(MODEL_ID)).toBe(false)
  })

  it('仅有用户时间规则 → true', () => {
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        timeOverrides: [makeTimeRule()],
      })
    )
    engine.refresh()
    expect(engine.hasTimePricing(MODEL_ID)).toBe(true)
  })

  it('仅有云端时间规则 → true', () => {
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        cloudTimeRules: new Map([[MODEL_ID, [makeCloudTimeRule()]]]),
      })
    )
    engine.refresh()
    expect(engine.hasTimePricing(MODEL_ID)).toBe(true)
  })

  it('用户 + 云端时间规则并存 → true', () => {
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        timeOverrides: [makeTimeRule()],
        cloudTimeRules: new Map([[MODEL_ID, [makeCloudTimeRule()]]]),
      })
    )
    engine.refresh()
    expect(engine.hasTimePricing(MODEL_ID)).toBe(true)
  })

  it('其他模型无规则 → false', () => {
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        timeOverrides: [makeTimeRule()],
      })
    )
    engine.refresh()
    expect(engine.hasTimePricing('other-model')).toBe(false)
  })
})

// ═══════════════════════════════════════════════════════════════════
// 7. resolveTier 档位匹配逻辑（通过 getPricingAtWithContext 间接测试）
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — resolveTier 档位匹配逻辑', () => {
  // 档位排序：[threshold=10000, threshold=50000]
  const EPOCH_T = 1700000000

  function createEngine() {
    const userTimeRule = makeTimeRule({
      startTime: EPOCH_T,
      endTime: EPOCH_T + 86400,
      contextTiers: [
        makeContextTier({ threshold: 10000, inputCostPerMillion: 10 }),
        makeContextTier({ threshold: 50000, inputCostPerMillion: 20 }),
      ],
    })
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        timeOverrides: [userTimeRule],
      })
    )
    engine.refresh()
    return engine
  }

  it('contextSize=5000 → 无匹配（10000 > 5000）', () => {
    const engine = createEngine()
    const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 5000)
    expect(result).not.toBeNull()
    // 未命中档位 → 使用时间规则平价 (makeTimeRule 默认 inputCostPerMillion=10.5)
    expect(result!.inputCostPerMillion).toBe(10.5)
  })

  it('contextSize=10000 → 命中 threshold=10000', () => {
    const engine = createEngine()
    const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 10000)
    expect(result).not.toBeNull()
    expect(result!.inputCostPerMillion).toBe(10)
  })

  it('contextSize=30000 → 命中 threshold=10000', () => {
    const engine = createEngine()
    const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 30000)
    expect(result).not.toBeNull()
    expect(result!.inputCostPerMillion).toBe(10)
  })

  it('contextSize=50000 → 命中 threshold=50000', () => {
    const engine = createEngine()
    const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 50000)
    expect(result).not.toBeNull()
    expect(result!.inputCostPerMillion).toBe(20)
  })

  it('contextSize=100000 → 命中 threshold=50000（最大档位）', () => {
    const engine = createEngine()
    const result = engine.getPricingAtWithContext(MODEL_ID, EPOCH_T + 1000, 100000)
    expect(result).not.toBeNull()
    expect(result!.inputCostPerMillion).toBe(20)
  })
})

// ═══════════════════════════════════════════════════════════════════
// 8. size、getAllPricing、getTimeRules、getOverrideTiers、getCloudTimeRules
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — getter 方法', () => {
  function createPopulatedEngine() {
    const cloudBase = [
      makeModelPricingRow({ modelId: 'model-a', displayName: 'Model A' }),
      makeModelPricingRow({ modelId: 'model-b', displayName: 'Model B' }),
      makeModelPricingRow({ modelId: MODEL_ID, displayName: 'Claude Sonnet 4' }),
    ]
    const timeOverride = makeTimeRule({ modelId: MODEL_ID })
    const cloudTimeRule = makeCloudTimeRule()
    const override = makePricingOverride({
      modelId: MODEL_ID,
      contextTiers: [makeContextTier()],
    })

    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase,
        overrides: [override],
        timeOverrides: [timeOverride],
        cloudTimeRules: new Map([[MODEL_ID, [cloudTimeRule]]]),
      })
    )
    engine.refresh()
    return { engine, cloudBase }
  }

  describe('size', () => {
    it('refresh 前为 0', () => {
      const engine = new PricingEngine(
        makeAppDbMock({ cloudBase: [makeModelPricingRow()] })
      )
      expect(engine.size).toBe(0)
    })

    it('refresh 后返回合并映射大小', () => {
      const { engine } = createPopulatedEngine()
      expect(engine.size).toBe(3)
    })
  })

  describe('getAllPricing', () => {
    it('返回所有合并定价的数组', () => {
      const { engine } = createPopulatedEngine()
      const all = engine.getAllPricing()
      expect(all).toHaveLength(3)
      const ids = all.map((p) => p.modelId).sort()
      expect(ids).toEqual(['claude-sonnet-4-20250514', 'model-a', 'model-b'])
    })

    it('覆盖的模型在列表中标记为 isOverride=true', () => {
      const { engine } = createPopulatedEngine()
      const overridden = engine.getAllPricing().find((p) => p.modelId === MODEL_ID)
      expect(overridden!.isOverride).toBe(true)
    })
  })

  describe('getTimeRules', () => {
    it('返回指定模型的用户时间规则', () => {
      const { engine } = createPopulatedEngine()
      const rules = engine.getTimeRules(MODEL_ID)
      expect(rules).toHaveLength(1)
      expect(rules[0].label).toBe('夜间折扣')
    })

    it('无规则的模型返回空数组', () => {
      const { engine } = createPopulatedEngine()
      expect(engine.getTimeRules('model-a')).toEqual([])
    })
  })

  describe('getOverrideTiers', () => {
    it('返回指定模型的覆盖上下文档位', () => {
      const { engine } = createPopulatedEngine()
      const tiers = engine.getOverrideTiers(MODEL_ID)
      expect(tiers).toHaveLength(1)
      expect(tiers[0].threshold).toBe(10000)
    })

    it('无覆盖档位的模型返回空数组', () => {
      const { engine } = createPopulatedEngine()
      expect(engine.getOverrideTiers('model-a')).toEqual([])
    })
  })

  describe('getCloudTimeRules', () => {
    it('返回指定模型的云端时间规则', () => {
      const { engine } = createPopulatedEngine()
      const rules = engine.getCloudTimeRules(MODEL_ID)
      expect(rules).toHaveLength(1)
      expect(rules[0].label).toBe('云端折扣')
    })

    it('无云端时间规则的模型返回空数组', () => {
      const { engine } = createPopulatedEngine()
      expect(engine.getCloudTimeRules('model-a')).toEqual([])
    })
  })
})

// ═══════════════════════════════════════════════════════════════════
// 9. 边界与防御性测试
// ═══════════════════════════════════════════════════════════════════
describe('PricingEngine — 边界与防御性', () => {
  it('云端基础加载失败（loadCloudPricing 抛异常）→ 不崩溃，merged 为空', () => {
    const brokenDb = {
      loadCloudPricing: () => { throw new Error('DB corrupted') },
      getAllOverrides: () => [],
      getAllTimeOverrides: () => [],
      getSetting: () => null,
    }
    const engine = new PricingEngine(brokenDb as any)
    engine.refresh()
    expect(engine.size).toBe(0)
    expect(engine.getPricing(MODEL_ID)).toBeNull()
  })

  it('contextSize 恰好等于 threshold → 包含在内（<= 判断）', () => {
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        overrides: [makePricingOverride({
          contextTiers: [makeContextTier({ threshold: 50000, inputCostPerMillion: 99 })],
        })],
      })
    )
    engine.refresh()
    const result = engine.getPricingAtWithContext(MODEL_ID, 0, 50000)
    expect(result).not.toBeNull()
    expect(result!.inputCostPerMillion).toBe(99)
  })

  it('contextSize 恰好小于 threshold 1 → 不命中', () => {
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        overrides: [makePricingOverride({
          contextTiers: [makeContextTier({ threshold: 50000, inputCostPerMillion: 99 })],
        })],
      })
    )
    engine.refresh()
    const result = engine.getPricingAtWithContext(MODEL_ID, 0, 49999)
    expect(result).not.toBeNull()
    // 未命中档位 → 回退到固定覆盖定价 (makePricingOverride 默认 30)
    expect(result!.inputCostPerMillion).toBe(30)
  })

  it('多个用户时间规则，第一个匹配的优先', () => {
    const rule1 = makeTimeRule({
      id: 1,
      startTime: 1700000000,
      endTime: 1700086400,
      inputCostPerMillion: 7,
    })
    const rule2 = makeTimeRule({
      id: 2,
      startTime: 1700000000,
      endTime: 1700086400,
      inputCostPerMillion: 14,
    })
    const engine = new PricingEngine(
      makeAppDbMock({
        cloudBase: [makeModelPricingRow()],
        timeOverrides: [rule1, rule2],
      })
    )
    engine.refresh()
    const result = engine.getPricingAt(MODEL_ID, 1700040000)
    expect(result).not.toBeNull()
    expect(result!.inputCostPerMillion).toBe(7) // 第一个匹配
  })

  it('calculateCost 对零定价不会产生 NaN', () => {
    const pricing = makeMergedPricing({
      inputCostPerMillion: 0,
      outputCostPerMillion: 0,
      cacheReadCostPerMillion: 0,
      cacheCreationCostPerMillion: 0,
    })
    const tokens = makeTokens()
    const engine = new PricingEngine(makeAppDbMock())
    expect(engine.calculateCost(pricing, tokens)).toBe(0)
  })
})
