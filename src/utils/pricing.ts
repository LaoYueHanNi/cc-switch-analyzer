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

/** 本地 ISO 周几（1=周一..7=周日），与后端 local_iso_weekday 一致 */
export function localDayOfWeek(epochSeconds: number, tzOffsetHours: number): number {
  const local = epochSeconds + tzOffsetHours * 3600
  const days = Math.floor(local / 86400)
  return (((days + 3) % 7) + 7) % 7 + 1
}

/** slot 是否在指定周几生效（daysOfWeek 缺省/空 = 每天） */
export function slotAppliesOnWeekday(slot: DailySlot, weekday: number): boolean {
  const days = slot.daysOfWeek
  if (!days || !days.length) return true
  return days.includes(weekday)
}

/** 两个 slot 的生效日是否相交（缺省/空 = 每天档，与任意档相交） */
export function slotDaysIntersect(a: DailySlot, b: DailySlot): boolean {
  const da = a.daysOfWeek
  const db = b.daysOfWeek
  if (!da || !da.length || !db || !db.length) return true
  return da.some(d => db.includes(d))
}

/** 匹配节点上的峰时 slot；未命中返回 null */
export function findMatchingDailySlot(
  slots: DailySlot[] | undefined,
  epochSeconds: number,
  tzOffsetHours: number
): DailySlot | null {
  if (!slots?.length) return null
  const minute = localMinuteOfDay(epochSeconds, tzOffsetHours)
  const weekday = localDayOfWeek(epochSeconds, tzOffsetHours)
  for (const slot of slots) {
    if (!slotAppliesOnWeekday(slot, weekday)) continue
    for (const w of slot.windows || []) {
      if (w.startMinute <= minute && minute < w.endMinute) {
        return slot
      }
    }
  }
  return null
}

/** 同一节点内 windows 是否重叠（半开区间）；仅当生效日相交时才算冲突 */
export function dailySlotsWindowsOverlap(slots: DailySlot[]): boolean {
  const ranges: Array<{ start: number; end: number; slot: DailySlot }> = []
  for (const slot of slots) {
    for (const w of slot.windows || []) {
      ranges.push({ start: w.startMinute, end: w.endMinute, slot })
    }
  }
  ranges.sort((a, b) => a.start - b.start)
  for (let i = 1; i < ranges.length; i++) {
    for (let j = 0; j < i; j++) {
      if (ranges[i].start < ranges[j].end && slotDaysIntersect(ranges[i].slot, ranges[j].slot)) {
        return true
      }
    }
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

const WEEKDAY_NAMES = ['', '周一', '周二', '周三', '周四', '周五', '周六', '周日']

/**
 * 格式化 daysOfWeek 为中文摘要，如 [1,2,3,4,5] → "周一至周五"。
 * 缺省/空/全 7 天 = 每天生效，返回空串。
 */
export function formatDaysOfWeek(days?: number[]): string {
  const unique = [...(days || [])].filter(d => d >= 1 && d <= 7)
  const sorted = [...new Set(unique)].sort((a, b) => a - b)
  if (!sorted.length || sorted.length >= 7) return ''
  const parts: string[] = []
  let start = sorted[0]
  let prev = sorted[0]
  for (let i = 1; i <= sorted.length; i++) {
    const cur = sorted[i]
    if (cur === prev + 1) {
      prev = cur
      continue
    }
    parts.push(start === prev ? WEEKDAY_NAMES[start] : `${WEEKDAY_NAMES[start]}至${WEEKDAY_NAMES[prev]}`)
    start = cur
    prev = cur
  }
  return parts.join('、')
}

/** 格式化峰时窗口摘要，如 周一至周五 08:00-12:00、14:00-18:00 */
export function formatDailySlotsSummary(slots: DailySlot[] | undefined): string {
  if (!slots?.length) return ''
  const groups: string[] = []
  for (const slot of slots) {
    const parts: string[] = []
    for (const w of slot.windows || []) {
      const sh = String(Math.floor(w.startMinute / 60)).padStart(2, '0')
      const sm = String(w.startMinute % 60).padStart(2, '0')
      const eh = String(Math.floor(w.endMinute / 60)).padStart(2, '0')
      const em = String(w.endMinute % 60).padStart(2, '0')
      parts.push(`${sh}:${sm}–${eh}:${em}`)
    }
    if (!parts.length) continue
    const days = formatDaysOfWeek(slot.daysOfWeek)
    groups.push(days ? `${days} ${parts.join('、')}` : parts.join('、'))
  }
  return groups.join('；')
}
