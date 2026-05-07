import type { AppDbService, CloudPricingTimeRule, ContextTier, ModelPricingRow, TimePricingRule } from './app-db'
import type { ExternalDbService } from './external-db'
import { fetchCloudPricing } from './cloud-pricing'

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

// 定价计算引擎 —— 四层定价优先级（上下文大小为子维度）
export class PricingEngine {
  private merged: Map<string, MergedPricing> = new Map()
  private timeOverrides: TimePricingRule[] = []
  private timeOverridesByModel: Map<string, TimePricingRule[]> = new Map()
  private cloudTimeRulesByModel: Map<string, CloudPricingTimeRule[]> = new Map()
  private overrideTiers: Map<string, ContextTier[]> = new Map()
  private timeRuleTiers: Map<number, ContextTier[]> = new Map()
  private cloudTimeRuleTiers: Map<string, ContextTier[]> = new Map()

  private appDb: AppDbService

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

  constructor(appDb: AppDbService) {
    this.appDb = appDb
  }

  // 刷新全部定价数据：云端基础 → 用户覆盖 → 云端时间规则 → 用户时间规则
  refresh(): void {
    // 1. 加载云端定价
    const { base, cloudTiers, cloudTimeRules } = this.loadCloudBase()

    // 2. 加载用户覆盖
    const overrides = this.loadOverrides()

    // 3. 合并
    this.merged = this.merge(base, cloudTiers, overrides)

    // 4. 加载云端时间规则
    this.cloudTimeRuleTiers = new Map()
    this.cloudTimeRulesByModel = new Map()
    for (const [modelId, rules] of cloudTimeRules) {
      for (const rule of rules) {
        if (rule.contextTiers?.length > 0) {
          const key = `${modelId}:${rule.startTime}:${rule.endTime}`
          this.cloudTimeRuleTiers.set(key, [...rule.contextTiers].sort((a, b) => a.threshold - b.threshold))
        }
      }
      this.cloudTimeRulesByModel.set(modelId, rules)
    }

    // 5. 加载用户时间规则
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

  // 加载云端基础定价：优先在线拉取，失败则读缓存
  private loadCloudBase(): { base: ModelPricingRow[], cloudTiers: Map<string, ContextTier[]>, cloudTimeRules: Map<string, CloudPricingTimeRule[]> } {
    try {
      const cached = this.appDb.loadCloudPricing()
      console.log(`[PRICING] 从缓存加载云端定价: ${cached.base.length} 个模型`)
      const cloudTiers = new Map<string, ContextTier[]>()
      for (const [modelId, tiers] of cached.tiers) {
        const sorted = [...tiers].sort((a, b) => a.threshold - b.threshold)
        cloudTiers.set(modelId, sorted)
      }
      return { base: cached.base, cloudTiers, cloudTimeRules: cached.cloudTimeRules }
    } catch (e) {
      console.log('[PRICING] 读取云端定价缓存失败:', e)
      return { base: [], cloudTiers: new Map(), cloudTimeRules: new Map() }
    }
  }

  // 异步拉取云端定价并更新缓存（启动时调用）
  async fetchAndCacheCloudPricing(): Promise<void> {
    try {
      const data = await fetchCloudPricing()
      console.log(`[PRICING] 云端定价拉取成功: ${data.models.length} 个模型, version=${data.version}`)
      const cachedVersion = this.appDb.getSetting('cloud_pricing_version')
      if (cachedVersion !== String(data.version)) {
        console.log(`[PRICING] 版本变化 ${cachedVersion || '(无)'} → ${data.version}, 更新缓存`)
        this.appDb.saveCloudPricing(data)
      } else {
        console.log(`[PRICING] 版本未变化 (${data.version}), 跳过缓存更新`)
      }
    } catch (e) {
      console.log('[PRICING] 云端定价拉取失败:', e)
    }
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

  // 合并：云端基础 + 云端档位 + 用户覆盖
  private merge(
    base: ModelPricingRow[],
    cloudTiers: Map<string, ContextTier[]>,
    overrides: Map<string, MergedPricing>
  ): Map<string, MergedPricing> {
    const result = new Map<string, MergedPricing>()

    // 云端基础定价（已是 RMB）
    for (const bp of base) {
      result.set(bp.modelId, {
        modelId: bp.modelId,
        displayName: bp.displayName || bp.modelId,
        inputCostPerMillion: bp.inputCostPerMillion,
        outputCostPerMillion: bp.outputCostPerMillion,
        cacheReadCostPerMillion: bp.cacheReadCostPerMillion,
        cacheCreationCostPerMillion: bp.cacheCreationCostPerMillion,
        isOverride: false
      })
    }

    // 云端基础上下文档位
    for (const [modelId, tiers] of cloudTiers) {
      if (!result.has(modelId)) continue
      if (!this.overrideTiers.has(modelId)) {
        this.overrideTiers.set(modelId, tiers)
      }
    }

    // 用户覆盖替换
    for (const [modelId, ov] of overrides) {
      const baseEntry = result.get(modelId)
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

  // 时间感知定价查询：用户时间规则 → 云端时间规则 → 固定定价
  getPricingAt(modelId: string, epochSeconds: number): MergedPricing | null {
    // 1. 用户自定义时间规则
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
    // 2. 云端时间规则
    const cloudRules = this.cloudTimeRulesByModel.get(modelId)
    if (cloudRules) {
      for (const rule of cloudRules) {
        if (rule.startTime <= epochSeconds && epochSeconds <= rule.endTime) {
          const baseEntry = this.merged.get(modelId)
          return {
            modelId,
            displayName: baseEntry?.displayName || modelId,
            inputCostPerMillion: rule.inputCostPerMillion,
            outputCostPerMillion: rule.outputCostPerMillion,
            cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
            cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
            isOverride: false
          }
        }
      }
    }
    // 回退到固定定价
    return this.merged.get(modelId) || null
  }

  // 上下文感知定价查询：用户时间规则 → 云端时间规则 → 覆盖档位 → 固定定价
  getPricingAtWithContext(modelId: string, epochSeconds: number, contextSize: number): MergedPricing | null {
    const displayName = this.merged.get(modelId)?.displayName || modelId

    // 1. 用户自定义时间规则
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

    // 2. 云端时间规则
    const cloudRules = this.cloudTimeRulesByModel.get(modelId)
    if (cloudRules) {
      for (const rule of cloudRules) {
        if (rule.startTime <= epochSeconds && epochSeconds <= rule.endTime) {
          const key = `${modelId}:${rule.startTime}:${rule.endTime}`
          const tiers = this.cloudTimeRuleTiers.get(key)
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
            isOverride: false
          }
        }
      }
    }

    // 3. 覆盖上下文档位
    const overrideTiers = this.overrideTiers.get(modelId)
    if (overrideTiers) {
      const tier = this.resolveTier(overrideTiers, contextSize)
      if (tier) return this.tierToMerged(modelId, tier, displayName)
    }

    // 4. 固定定价
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

  // 获取所有合并后的定价列表
  getAllPricing(): MergedPricing[] {
    return Array.from(this.merged.values())
  }

  // 获取某模型的时间规则
  getTimeRules(modelId: string): TimePricingRule[] {
    return this.timeOverridesByModel.get(modelId) || []
  }

  // 获取某模型的覆盖上下文档位
  getOverrideTiers(modelId: string): ContextTier[] {
    return this.overrideTiers.get(modelId) || []
  }

  // 返回命中的上下文档位 threshold，无命中返回 null
  getMatchedTierThreshold(modelId: string, epochSeconds: number, contextSize: number): number | null {
    // 1. 用户时间规则的档位
    const rules = this.timeOverridesByModel.get(modelId)
    if (rules) {
      for (const rule of rules) {
        if (rule.startTime <= epochSeconds && epochSeconds <= rule.endTime) {
          const tiers = this.timeRuleTiers.get(rule.id)
          if (tiers) {
            const tier = PricingEngine.resolveTierStatic(tiers, contextSize)
            if (tier) return tier.threshold
          }
          return null
        }
      }
    }
    // 2. 云端时间规则的档位
    const cloudRules = this.cloudTimeRulesByModel.get(modelId)
    if (cloudRules) {
      for (const rule of cloudRules) {
        if (rule.startTime <= epochSeconds && epochSeconds <= rule.endTime) {
          const key = `${modelId}:${rule.startTime}:${rule.endTime}`
          const tiers = this.cloudTimeRuleTiers.get(key)
          if (tiers) {
            const tier = PricingEngine.resolveTierStatic(tiers, contextSize)
            if (tier) return tier.threshold
          }
          return null
        }
      }
    }
    // 3. 覆盖档位
    const overrideTiers = this.overrideTiers.get(modelId)
    if (overrideTiers) {
      const tier = PricingEngine.resolveTierStatic(overrideTiers, contextSize)
      if (tier) return tier.threshold
    }
    return null
  }

  private static resolveTierStatic(tiers: ContextTier[], contextSize: number): ContextTier | null {
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

  // 检查是否有时间定价（用户规则或云端规则）
  hasTimePricing(modelId: string): boolean {
    const rules = this.timeOverridesByModel.get(modelId)
    if (rules && rules.length > 0) return true
    const cloudRules = this.cloudTimeRulesByModel.get(modelId)
    return cloudRules !== undefined && cloudRules.length > 0
  }

  // 获取某模型的云端时间规则
  getCloudTimeRules(modelId: string): CloudPricingTimeRule[] {
    return this.cloudTimeRulesByModel.get(modelId) || []
  }

  // 获取合并映射大小
  get size(): number {
    return this.merged.size
  }
}
