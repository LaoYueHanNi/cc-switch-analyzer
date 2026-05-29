import type { PricingData, ContextTier } from '@/types/pricing'

/** 单价字段名 */
export type RateField = 'inputCostPerMillion' | 'outputCostPerMillion' | 'cacheReadCostPerMillion' | 'cacheCreationCostPerMillion'

/** 所有单价字段 */
export const ALL_RATE_FIELDS: RateField[] = [
  'inputCostPerMillion',
  'outputCostPerMillion',
  'cacheReadCostPerMillion',
  'cacheCreationCostPerMillion'
]

/**
 * 在上下文档位中找到匹配的档位。
 * 规则：threshold <= contextSize 的最大档位。
 */
export function resolveTier(tiers: ContextTier[], contextSize: number): ContextTier | null {
  let best: ContextTier | null = null
  for (const tier of tiers) {
    if (tier.threshold <= contextSize && (!best || tier.threshold > best.threshold)) {
      best = tier
    }
  }
  return best
}

/** getActiveRate 返回的单价结构 */
export interface ActiveRates {
  inputRate: number
  outputRate: number
  cacheReadRate: number
  cacheCreationRate: number
}

/**
 * 获取当前有效的定价单价，按优先级：
 * 1. 用户时间规则（最具体的匹配）
 * 2. 云端时间规则
 * 3. 上下文档位覆盖
 * 4. 基础定价（含用户覆盖）
 *
 * @param pricing     模型定价数据
 * @param contextSize 可选，上下文大小（input + cacheRead），用于查找上下文档位
 */
export function getActiveRate(pricing: PricingData | undefined, contextSize?: number, timestamp?: number): ActiveRates {
  if (!pricing) return { inputRate: 0, outputRate: 0, cacheReadRate: 0, cacheCreationRate: 0 }

  const now = timestamp ?? Math.floor(Date.now() / 1000)

  // 1. 时间规则（用户 > 云端）
  const rule = pricing.timeRules?.find(r => now >= r.startTime && now <= r.endTime)
    || pricing.cloudTimeRules?.find(r => now >= r.startTime && now <= r.endTime)

  if (rule) {
    const tiers = rule.contextTiers || []
    const tier = contextSize != null ? resolveTier(tiers, contextSize) : null
    const source = tier || rule
    return {
      inputRate: source.inputCostPerMillion,
      outputRate: source.outputCostPerMillion,
      cacheReadRate: source.cacheReadCostPerMillion,
      cacheCreationRate: source.cacheCreationCostPerMillion
    }
  }

  // 2. 上下文档位覆盖
  const tiers = pricing.contextTiers || []
  const tier = contextSize != null ? resolveTier(tiers, contextSize) : null
  if (tier) {
    return {
      inputRate: tier.inputCostPerMillion,
      outputRate: tier.outputCostPerMillion,
      cacheReadRate: tier.cacheReadCostPerMillion,
      cacheCreationRate: tier.cacheCreationCostPerMillion
    }
  }

  // 3. 基础定价
  return {
    inputRate: pricing.inputCostPerMillion || 0,
    outputRate: pricing.outputCostPerMillion || 0,
    cacheReadRate: pricing.cacheReadCostPerMillion || 0,
    cacheCreationRate: pricing.cacheCreationCostPerMillion || 0
  }
}

/** resolveBucketPricingRule 返回的规则标识 */
export interface ResolvedPricingRule {
  key: string
  label: string
  startTime?: number
  endTime?: number
  isTimeRule: boolean
}

/**
 * 为 CompareBucket 解析命中的定价规则和费率。
 * 逻辑与后端 get_pricing_at_with_context 一致。
 */
export function resolveBucketPricingRule(
  pricing: PricingData,
  epoch: number,
  threshold: number
): { rule: ResolvedPricingRule; rates: ActiveRates } {
  const makeRates = (r: { inputCostPerMillion: number; outputCostPerMillion: number; cacheReadCostPerMillion: number; cacheCreationCostPerMillion: number }): ActiveRates => ({
    inputRate: r.inputCostPerMillion,
    outputRate: r.outputCostPerMillion,
    cacheReadRate: r.cacheReadCostPerMillion,
    cacheCreationRate: r.cacheCreationCostPerMillion
  })

  // 1. 用户时间规则
  for (const rule of pricing.timeRules || []) {
    if (epoch >= rule.startTime && epoch <= rule.endTime) {
      const tier = threshold > 0 ? (rule.contextTiers || []).find(t => t.threshold === threshold) : null
      return {
        rule: { key: `tu-${rule.id}`, label: rule.label || '时段定价', startTime: rule.startTime, endTime: rule.endTime, isTimeRule: true },
        rates: makeRates(tier || rule)
      }
    }
  }

  // 2. 云端时间规则
  for (const rule of pricing.cloudTimeRules || []) {
    if (epoch >= rule.startTime && epoch <= rule.endTime) {
      const tier = threshold > 0 ? (rule.contextTiers || []).find(t => t.threshold === threshold) : null
      return {
        rule: { key: `tc-${rule.startTime}-${rule.endTime}`, label: rule.label || '时段定价', startTime: rule.startTime, endTime: rule.endTime, isTimeRule: true },
        rates: makeRates(tier || rule)
      }
    }
  }

  // 3. 静态上下文档位
  if (threshold > 0) {
    const tier = (pricing.contextTiers || []).find(t => t.threshold === threshold)
    if (tier) {
      return {
        rule: { key: 'base', label: '当前定价', isTimeRule: false },
        rates: makeRates(tier)
      }
    }
  }

  // 4. 基础定价
  return {
    rule: { key: 'base', label: '当前定价', isTimeRule: false },
    rates: makeRates(pricing)
  }
}
