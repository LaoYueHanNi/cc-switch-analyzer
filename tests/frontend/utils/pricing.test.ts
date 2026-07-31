import { describe, expect, it } from 'vitest'
import {
  getActiveRate,
  findMatchingDailySlot,
  dailySlotsWindowsOverlap,
  localMinuteOfDay,
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
})
