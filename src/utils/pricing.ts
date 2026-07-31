import type { PricingData, ContextTier, DailySlot } from '@/types/pricing'

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

/** 本地当天分钟数（与后端 local_minute_of_day 一致） */
export function localMinuteOfDay(epochSeconds: number, tzOffsetHours: number): number {
  const local = epochSeconds + tzOffsetHours * 3600
  const sod = ((local % 86400) + 86400) % 86400
  return Math.floor(sod / 60)
}

/** 匹配节点上的峰时 slot；未命中返回 null */
export function findMatchingDailySlot(
  slots: DailySlot[] | undefined,
  epochSeconds: number,
  tzOffsetHours: number
): DailySlot | null {
  if (!slots?.length) return null
  const minute = localMinuteOfDay(epochSeconds, tzOffsetHours)
  for (const slot of slots) {
    for (const w of slot.windows || []) {
      if (w.startMinute <= minute && minute < w.endMinute) {
        return slot
      }
    }
  }
  return null
}

/** 同一节点内 windows 是否重叠（半开区间） */
export function dailySlotsWindowsOverlap(slots: DailySlot[]): boolean {
  const ranges: Array<{ start: number; end: number }> = []
  for (const slot of slots) {
    for (const w of slot.windows || []) {
      ranges.push({ start: w.startMinute, end: w.endMinute })
    }
  }
  ranges.sort((a, b) => a.start - b.start)
  for (let i = 1; i < ranges.length; i++) {
    if (ranges[i].start < ranges[i - 1].end) return true
  }
  return false
}

type RateSource = {
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  dailySlots?: DailySlot[]
}

function applyDailySlot(source: RateSource, epoch: number, tzOffset: number): RateSource {
  const slot = findMatchingDailySlot(source.dailySlots, epoch, tzOffset)
  return slot || source
}

/** getActiveRate 返回的单价结构 */
export interface ActiveRates {
  inputRate: number
  outputRate: number
  cacheReadRate: number
  cacheCreationRate: number
}

function toActiveRates(r: RateSource): ActiveRates {
  return {
    inputRate: r.inputCostPerMillion || 0,
    outputRate: r.outputCostPerMillion || 0,
    cacheReadRate: r.cacheReadCostPerMillion || 0,
    cacheCreationRate: r.cacheCreationCostPerMillion || 0
  }
}

/**
 * 获取当前有效的定价单价（三层互斥：容器 → 节点 → 峰谷）
 */
export function getActiveRate(
  pricing: PricingData | undefined,
  contextSize?: number,
  timestamp?: number,
  tzOffsetHours?: number
): ActiveRates {
  if (!pricing) return { inputRate: 0, outputRate: 0, cacheReadRate: 0, cacheCreationRate: 0 }

  const now = timestamp ?? Math.floor(Date.now() / 1000)
  const tz = tzOffsetHours ?? -new Date().getTimezoneOffset() / 60

  // 1. 时间规则容器（用户 > 云端）
  const rule = pricing.timeRules?.find(r => now >= r.startTime && now <= r.endTime)
    || pricing.cloudTimeRules?.find(r => now >= r.startTime && now <= r.endTime)

  if (rule) {
    const tiers = rule.contextTiers || []
    const tier = contextSize != null ? resolveTier(tiers, contextSize) : null
    const node: RateSource = tier || {
      inputCostPerMillion: rule.inputCostPerMillion,
      outputCostPerMillion: rule.outputCostPerMillion,
      cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
      cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
      dailySlots: rule.dailySlots
    }
    return toActiveRates(applyDailySlot(node, now, tz))
  }

  // 2. 模型根容器：先档位再峰谷
  const tiers = pricing.contextTiers || []
  const tier = contextSize != null ? resolveTier(tiers, contextSize) : null
  if (tier) {
    return toActiveRates(applyDailySlot(tier, now, tz))
  }

  return toActiveRates(applyDailySlot({
    inputCostPerMillion: pricing.inputCostPerMillion || 0,
    outputCostPerMillion: pricing.outputCostPerMillion || 0,
    cacheReadCostPerMillion: pricing.cacheReadCostPerMillion || 0,
    cacheCreationCostPerMillion: pricing.cacheCreationCostPerMillion || 0,
    dailySlots: pricing.dailySlots
  }, now, tz))
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
 * 逻辑与后端 get_pricing_at_with_context 一致：先档位再峰谷。
 */
export function resolveBucketPricingRule(
  pricing: PricingData,
  epoch: number,
  threshold: number,
  tzOffsetHours?: number
): { rule: ResolvedPricingRule; rates: ActiveRates } {
  const tz = tzOffsetHours ?? -new Date().getTimezoneOffset() / 60

  const finalize = (
    meta: ResolvedPricingRule,
    node: RateSource
  ): { rule: ResolvedPricingRule; rates: ActiveRates } => ({
    rule: meta,
    rates: toActiveRates(applyDailySlot(node, epoch, tz))
  })

  // 1. 用户时间规则
  for (const rule of pricing.timeRules || []) {
    if (epoch >= rule.startTime && epoch <= rule.endTime) {
      const tier = threshold > 0 ? (rule.contextTiers || []).find(t => t.threshold === threshold) : null
      const node: RateSource = tier || {
        inputCostPerMillion: rule.inputCostPerMillion,
        outputCostPerMillion: rule.outputCostPerMillion,
        cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
        cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
        dailySlots: rule.dailySlots
      }
      return finalize(
        { key: `tu-${rule.id}`, label: rule.label || '时段定价', startTime: rule.startTime, endTime: rule.endTime, isTimeRule: true },
        node
      )
    }
  }

  // 2. 云端时间规则
  for (const rule of pricing.cloudTimeRules || []) {
    if (epoch >= rule.startTime && epoch <= rule.endTime) {
      const tier = threshold > 0 ? (rule.contextTiers || []).find(t => t.threshold === threshold) : null
      const node: RateSource = tier || {
        inputCostPerMillion: rule.inputCostPerMillion,
        outputCostPerMillion: rule.outputCostPerMillion,
        cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
        cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
        dailySlots: rule.dailySlots
      }
      return finalize(
        { key: `tc-${rule.startTime}-${rule.endTime}`, label: rule.label || '时段定价', startTime: rule.startTime, endTime: rule.endTime, isTimeRule: true },
        node
      )
    }
  }

  // 3. 静态上下文档位
  if (threshold > 0) {
    const tier = (pricing.contextTiers || []).find(t => t.threshold === threshold)
    if (tier) {
      return finalize({ key: 'base', label: '当前定价', isTimeRule: false }, tier)
    }
  }

  // 4. 基础定价
  return finalize({ key: 'base', label: '当前定价', isTimeRule: false }, {
    inputCostPerMillion: pricing.inputCostPerMillion,
    outputCostPerMillion: pricing.outputCostPerMillion,
    cacheReadCostPerMillion: pricing.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: pricing.cacheCreationCostPerMillion,
    dailySlots: pricing.dailySlots
  })
}

/** 格式化峰时窗口摘要，如 08:00-12:00,14:00-18:00 */
export function formatDailySlotsSummary(slots: DailySlot[] | undefined): string {
  if (!slots?.length) return ''
  const parts: string[] = []
  for (const slot of slots) {
    for (const w of slot.windows || []) {
      const sh = String(Math.floor(w.startMinute / 60)).padStart(2, '0')
      const sm = String(w.startMinute % 60).padStart(2, '0')
      const eh = String(Math.floor(w.endMinute / 60)).padStart(2, '0')
      const em = String(w.endMinute % 60).padStart(2, '0')
      parts.push(`${sh}:${sm}–${eh}:${em}`)
    }
  }
  return parts.join('、')
}
