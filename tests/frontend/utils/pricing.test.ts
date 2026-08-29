import { describe, expect, it } from 'vitest'
import {
  getActiveRate,
  findMatchingDailySlot,
  dailySlotsWindowsOverlap,
  localMinuteOfDay,
  localDayOfWeek,
  slotDaysIntersect,
  formatDaysOfWeek,
  formatDailySlotsSummary,
  resolveBucketPricingRule
} from '@/utils/pricing'
import type { PricingData, DailySlot } from '@/types/pricing'

const peak: DailySlot = {
  label: '峰时',
  windows: [
    { startMinute: 480, endMinute: 720 },
    { startMinute: 840, endMinute: 1080 }
  ],
  inputCostPerMillion: 20,
  outputCostPerMillion: 120,
  cacheReadCostPerMillion: 2,
  cacheCreationCostPerMillion: 25
}

const pricing: PricingData = {
  modelId: 'm',
  inputCostPerMillion: 14,
  outputCostPerMillion: 84,
  cacheReadCostPerMillion: 1.4,
  cacheCreationCostPerMillion: 17.5,
  isOverride: false,
  hasTimePricing: true,
  isUsed: true,
  aliases: [],
  userAliases: [],
  dailySlots: [{ ...peak, inputCostPerMillion: 16, label: '模型根-峰时' }],
  contextTiers: [{
    threshold: 128000,
    inputCostPerMillion: 28,
    outputCostPerMillion: 168,
    cacheReadCostPerMillion: 2.8,
    cacheCreationCostPerMillion: 35,
    dailySlots: [{ ...peak, inputCostPerMillion: 32, label: '模型128K-峰时' }]
  }],
  timeRules: [{
    id: 1,
    modelId: 'm',
    label: '原价',
    startTime: 0,
    endTime: 1769875199,
    inputCostPerMillion: 17.5,
    outputCostPerMillion: 105,
    cacheReadCostPerMillion: 1.75,
    cacheCreationCostPerMillion: 21.875,
    dailySlots: [peak],
    contextTiers: [{
      threshold: 128000,
      inputCostPerMillion: 35,
      outputCostPerMillion: 210,
      cacheReadCostPerMillion: 3.5,
      cacheCreationCostPerMillion: 43.75,
      dailySlots: [{ ...peak, inputCostPerMillion: 40, label: '原价128K-峰时' }]
    }]
  }],
  cloudTimeRules: []
}

describe('dailySlots pricing', () => {
  it('localMinuteOfDay matches half-open windows', () => {
    expect(localMinuteOfDay(10 * 3600, 0)).toBe(600)
    expect(findMatchingDailySlot([peak], 10 * 3600, 0)?.inputCostPerMillion).toBe(20)
    expect(findMatchingDailySlot([peak], 12 * 3600, 0)).toBeNull()
  })

  it('detects overlapping windows', () => {
    expect(dailySlotsWindowsOverlap([peak])).toBe(false)
    expect(dailySlotsWindowsOverlap([{
      ...peak,
      windows: [
        { startMinute: 480, endMinute: 720 },
        { startMinute: 700, endMinute: 800 }
      ]
    }])).toBe(true)
  })

  it('A paths: time rule root then peak/valley; tier does not borrow root peak', () => {
    const tz = 8
    const day = 1736899200 // UTC midnight
    const peakEpoch = day + (10 - tz) * 3600
    const offEpoch = day + (13 - tz) * 3600

    expect(getActiveRate(pricing, 1000, peakEpoch, tz).inputRate).toBe(20)
    expect(getActiveRate(pricing, 1000, offEpoch, tz).inputRate).toBe(17.5)
    expect(getActiveRate(pricing, 128000, peakEpoch, tz).inputRate).toBe(40)
    expect(getActiveRate(pricing, 128000, offEpoch, tz).inputRate).toBe(35)
  })

  it('C paths: no time rule falls back to model root dailySlots', () => {
    const tz = 8
    const after = 1770566400
    const peakEpoch = after + 10 * 3600
    const offEpoch = after + 13 * 3600
    expect(getActiveRate(pricing, 1000, peakEpoch, tz).inputRate).toBe(16)
    expect(getActiveRate(pricing, 1000, offEpoch, tz).inputRate).toBe(14)
    expect(getActiveRate(pricing, 128000, peakEpoch, tz).inputRate).toBe(32)
  })

  it('resolveBucketPricingRule applies slot after tier', () => {
    const tz = 8
    const day = 1736899200
    const peakEpoch = day + (10 - tz) * 3600
    const { rates } = resolveBucketPricingRule(pricing, peakEpoch, 128000, tz)
    expect(rates.inputRate).toBe(40)
  })

  it('localDayOfWeek returns ISO weekdays', () => {
    expect(localDayOfWeek(0, 0)).toBe(4) // 1970-01-01 周四
    const mondayUtc0 = 1767571200 // 2026-01-05 周一
    expect(localDayOfWeek(mondayUtc0, 0)).toBe(1)
    expect(localDayOfWeek(mondayUtc0 - 86400, 0)).toBe(7)
    // 本地周一 00:00 = UTC 周日 16:00（tz=8）
    expect(localDayOfWeek(mondayUtc0 - 8 * 3600, 8)).toBe(1)
    expect(localDayOfWeek(mondayUtc0 - 8 * 3600 - 1, 8)).toBe(7)
  })

  it('findMatchingDailySlot respects daysOfWeek', () => {
    const tz = 8
    const mondayUtc0 = 1767571200 // 2026-01-05 周一
    const workday: DailySlot = { ...peak, daysOfWeek: [1, 2, 3, 4, 5] }
    // 周一本地 10:00 命中
    expect(findMatchingDailySlot([workday], mondayUtc0 + (10 - tz) * 3600, tz)).toBe(workday)
    // 周六本地 10:00 不命中
    const saturday = mondayUtc0 + 5 * 86400 + (10 - tz) * 3600
    expect(findMatchingDailySlot([workday], saturday, tz)).toBeNull()
    // 缺省/空 = 每天
    expect(findMatchingDailySlot([peak], saturday, tz)).toBe(peak)
    expect(findMatchingDailySlot([{ ...peak, daysOfWeek: [] }], saturday, tz)).not.toBeNull()
  })

  it('dailySlotsWindowsOverlap only conflicts on intersecting days', () => {
    // 工作日峰与周末峰窗口重叠但生效日不相交 → 不冲突
    const workday: DailySlot = { ...peak, daysOfWeek: [1, 2, 3, 4, 5] }
    const weekend: DailySlot = { ...peak, windows: [{ startMinute: 700, endMinute: 800 }], daysOfWeek: [6, 7] }
    expect(dailySlotsWindowsOverlap([workday, weekend])).toBe(false)
    // 无 daysOfWeek 的档视为每天，与任意档相交
    expect(dailySlotsWindowsOverlap([workday, { ...peak, windows: [{ startMinute: 700, endMinute: 800 }] }])).toBe(true)
    expect(slotDaysIntersect(workday, weekend)).toBe(false)
    expect(slotDaysIntersect(peak, weekend)).toBe(true)
  })

  it('formatDaysOfWeek compresses consecutive days', () => {
    expect(formatDaysOfWeek([1, 2, 3, 4, 5])).toBe('周一至周五')
    expect(formatDaysOfWeek([6, 7])).toBe('周六至周日')
    expect(formatDaysOfWeek([2, 3, 4])).toBe('周二至周四')
    expect(formatDaysOfWeek([1, 3])).toBe('周一、周三')
    expect(formatDaysOfWeek([1, 2, 3, 4, 5, 6, 7])).toBe('')
    expect(formatDaysOfWeek([])).toBe('')
    expect(formatDaysOfWeek(undefined)).toBe('')
  })

  it('formatDailySlotsSummary prefixes weekday restriction', () => {
    expect(formatDailySlotsSummary([peak])).toBe('08:00–12:00、14:00–18:00')
    const workday: DailySlot = { ...peak, daysOfWeek: [1, 2, 3, 4, 5] }
    expect(formatDailySlotsSummary([workday])).toBe('周一至周五 08:00–12:00、14:00–18:00')
  })
})
