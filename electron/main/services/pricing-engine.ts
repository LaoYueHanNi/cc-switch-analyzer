import type { ExternalDbService, ModelPricing, FilterParams } from './external-db'
import type { AppDbService, ContextTier } from './app-db'
import { DEFAULT_EXCHANGE_RATE } from '../utils/constants'

// 合并后的定价（包含 isOverride 标记）
export interface MergedPricing {
  modelId: string
  displayName: string
  inputCostPerMillion: number     // RMB
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  isOverride: boolean
}

// Token 用量维度
export interface TokenDimensions {
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}

// 定价计算引擎 —— 三层定价优先级（上下文大小为子维度）
export class PricingEngine {
  private merged: Map<string, MergedPricing> = new Map()
  private timeOverrides: any[] = []  // TimePricingRule[]
  private timeOverridesByModel: Map<string, any[]> = new Map()
  private exchangeRate: number = DEFAULT_EXCHANGE_RATE
  private overrideTiers: Map<string, ContextTier[]> = new Map()
  private timeRuleTiers: Map<number, ContextTier[]> = new Map()

  private externalDb: ExternalDbService
  private appDb: AppDbService

  private round(v: number): number {
    return Math.round(v * 1e6) / 1e6
  }

  private resolveTier(tiers: ContextTier[], contextSize: number): ContextTier | null {
    let best: ContextTier | null = null
    for (const tier of tiers) {
      if (tier.threshold <= contextSize) {
        best = tier
      } else {
        break
      }
    }
    return best
  }

  private tierToMerged(modelId: string, tier: ContextTier, displayName: string): MergedPricing {
    return {
      modelId,
      displayName,
      inputCostPerMillion: tier.inputCostPerMillion,
      outputCostPerMillion: tier.outputCostPerMillion,
      cacheReadCostPerMillion: tier.cacheReadCostPerMillion,
      cacheCreationCostPerMillion: tier.cacheCreationCostPerMillion,
      isOverride: true
    }
  }

  constructor(externalDb: ExternalDbService, appDb: AppDbService) {
    this.externalDb = externalDb
    this.appDb = appDb
  }

  // 刷新全部定价数据：汇率 → 基础定价 × 汇率 → 覆盖 → 时间规则
  refresh(): void {
    // 1. 加载汇率
    this.exchangeRate = this.appDb.getExchangeRate()

    // 2. 加载基础定价（外部 DB 可能尚未打开）
    const base = this.externalDb.isOpen ? this.loadBasePricing() : new Map()

    // 3. 加载用户覆盖（已是 RMB）
    const overrides = this.loadOverrides()

    // 4. 合并：覆盖替换基础
    this.merged = this.merge(base, overrides)

    // 5. 加载时间规则并分组 + 提取上下文档位
    this.timeRuleTiers = new Map()
    this.timeOverrides = this.appDb.getAllTimeOverrides()
    this.timeOverridesByModel = new Map()
    for (const rule of this.timeOverrides) {
      if (rule.contextTiers?.length > 0) {
        const sorted = [...rule.contextTiers].sort((a: any, b: any) => a.threshold - b.threshold)
        this.timeRuleTiers.set(rule.id, sorted)
      }
      const list = this.timeOverridesByModel.get(rule.modelId) || []
      list.push(rule)
      this.timeOverridesByModel.set(rule.modelId, list)
    }
  }

  // 加载基础定价（外部数据库）并转换为 RMB
  private loadBasePricing(): Map<string, MergedPricing> {
    const basePricing = this.externalDb.getBasePricing()
    const map = new Map<string, MergedPricing>()
    for (const bp of basePricing) {
      map.set(bp.modelId, {
        modelId: bp.modelId,
        displayName: bp.displayName || bp.modelId,
        inputCostPerMillion: this.round(bp.inputCostPerMillion * this.exchangeRate),
        outputCostPerMillion: this.round(bp.outputCostPerMillion * this.exchangeRate),
        cacheReadCostPerMillion: this.round(bp.cacheReadCostPerMillion * this.exchangeRate),
        cacheCreationCostPerMillion: this.round(bp.cacheCreationCostPerMillion * this.exchangeRate),
        isOverride: false
      })
    }
    return map
  }

  // 加载用户覆盖（RMB 直存）
  private loadOverrides(): Map<string, MergedPricing> {
    const rawOverrides = this.appDb.getAllOverrides()
    this.overrideTiers = new Map()
    const map = new Map<string, MergedPricing>()
    for (const ov of rawOverrides) {
      const baseEntry = this.merged.get(ov.modelId)
      if (ov.contextTiers?.length > 0) {
        const sorted = [...ov.contextTiers].sort((a: any, b: any) => a.threshold - b.threshold)
        this.overrideTiers.set(ov.modelId, sorted)
      }
      map.set(ov.modelId, {
        modelId: ov.modelId,
        displayName: baseEntry?.displayName || ov.modelId,
        inputCostPerMillion: ov.inputCostPerMillion,
        outputCostPerMillion: ov.outputCostPerMillion,
        cacheReadCostPerMillion: ov.cacheReadCostPerMillion,
        cacheCreationCostPerMillion: ov.cacheCreationCostPerMillion,
        isOverride: true
      })
    }
    return map
  }

  // 合并：基础 + 覆盖覆盖
  private merge(base: Map<string, MergedPricing>, overrides: Map<string, MergedPricing>): Map<string, MergedPricing> {
    const result = new Map(base)
    for (const [modelId, ov] of overrides) {
      const baseEntry = base.get(modelId)
      result.set(modelId, {
        ...ov,
        displayName: baseEntry?.displayName || ov.modelId,
        isOverride: true
      })
    }
    return result
  }

  // 获取固定合并定价（忽略时间规则）
  getPricing(modelId: string): MergedPricing | null {
    return this.merged.get(modelId) || null
  }

  // 时间感知定价查询：优先时间规则 → 固定定价
  getPricingAt(modelId: string, epochSeconds: number): MergedPricing | null {
    const rules = this.timeOverridesByModel.get(modelId)
    if (rules) {
      for (const rule of rules) {
        if (rule.startTime <= epochSeconds && epochSeconds <= rule.endTime) {
          const baseEntry = this.merged.get(modelId)
          return {
            modelId,
            displayName: baseEntry?.displayName || modelId,
            inputCostPerMillion: rule.inputCostPerMillion,
            outputCostPerMillion: rule.outputCostPerMillion,
            cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
            cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
            isOverride: true
          }
        }
      }
    }
    // 回退到固定定价
    return this.merged.get(modelId) || null
  }

  // 上下文感知定价查询：时间规则 → 覆盖档位 → 固定定价
  getPricingAtWithContext(modelId: string, epochSeconds: number, contextSize: number): MergedPricing | null {
    const displayName = this.merged.get(modelId)?.displayName || modelId

    // 1. 时间规则优先
    const rules = this.timeOverridesByModel.get(modelId)
    if (rules) {
      for (const rule of rules) {
        if (rule.startTime <= epochSeconds && epochSeconds <= rule.endTime) {
          const tiers = this.timeRuleTiers.get(rule.id)
          if (tiers) {
            const tier = this.resolveTier(tiers, contextSize)
            if (tier) return this.tierToMerged(modelId, tier, displayName)
          }
          return {
            modelId,
            displayName,
            inputCostPerMillion: rule.inputCostPerMillion,
            outputCostPerMillion: rule.outputCostPerMillion,
            cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
            cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
            isOverride: true
          }
        }
      }
    }

    // 2. 覆盖上下文档位
    const overrideTiers = this.overrideTiers.get(modelId)
    if (overrideTiers) {
      const tier = this.resolveTier(overrideTiers, contextSize)
      if (tier) return this.tierToMerged(modelId, tier, displayName)
    }

    // 3. 固定定价
    return this.merged.get(modelId) || null
  }

  // 费用计算
  calculateCost(pricing: MergedPricing, tokens: TokenDimensions): number {
    return (
      tokens.input * pricing.inputCostPerMillion +
      tokens.output * pricing.outputCostPerMillion +
      tokens.cacheRead * pricing.cacheReadCostPerMillion +
      tokens.cacheCreation * pricing.cacheCreationCostPerMillion
    ) / 1_000_000
  }

  // 计算四维费用分解
  calculateCostBreakdown(pricing: MergedPricing, tokens: TokenDimensions): [number, number, number, number] {
    return [
      tokens.input * pricing.inputCostPerMillion / 1_000_000,
      tokens.output * pricing.outputCostPerMillion / 1_000_000,
      tokens.cacheRead * pricing.cacheReadCostPerMillion / 1_000_000,
      tokens.cacheCreation * pricing.cacheCreationCostPerMillion / 1_000_000
    ]
  }

  // 获取当前汇率
  getExchangeRate(): number {
    return this.exchangeRate
  }

  // 获取所有合并后的定价列表
  getAllPricing(): MergedPricing[] {
    return Array.from(this.merged.values())
  }

  // 获取某模型的时间规则
  getTimeRules(modelId: string): any[] {
    return this.timeOverridesByModel.get(modelId) || []
  }

  // 获取某模型的覆盖上下文档位
  getOverrideTiers(modelId: string): ContextTier[] {
    return this.overrideTiers.get(modelId) || []
  }

  // 返回命中的上下文档位 threshold，无命中返回 null
  getMatchedTierThreshold(modelId: string, epochSeconds: number, contextSize: number): number | null {
    // 1. 时间规则的档位
    const rules = this.timeOverridesByModel.get(modelId)
    if (rules) {
      for (const rule of rules) {
        if (rule.startTime <= epochSeconds && epochSeconds <= rule.endTime) {
          const tiers = this.timeRuleTiers.get(rule.id)
          if (tiers) {
            const tier = PricingEngine.resolveTier(tiers, contextSize)
            if (tier) return tier.threshold
          }
          return null
        }
      }
    }
    // 2. 覆盖档位
    const overrideTiers = this.overrideTiers.get(modelId)
    if (overrideTiers) {
      const tier = PricingEngine.resolveTier(overrideTiers, contextSize)
      if (tier) return tier.threshold
    }
    return null
  }

  // 检查是否有时间定价
  hasTimePricing(modelId: string): boolean {
    const rules = this.timeOverridesByModel.get(modelId)
    return rules !== undefined && rules.length > 0
  }

  // 获取合并映射大小
  get size(): number {
    return this.merged.size
  }
}
