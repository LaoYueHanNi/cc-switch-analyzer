import { describe, expect, it, vi } from 'vitest'
import { createModelResolver } from '@/utils/modelResolver'
import { matchesModelSearch } from '@/utils/modelSearch'
import type { PricingData } from '@/types/pricing'

function pricing(partial: Partial<PricingData> = {}): PricingData {
  return {
    modelId: 'Claude-Opus-4.7',
    inputCostPerMillion: 0,
    outputCostPerMillion: 0,
    cacheReadCostPerMillion: 0,
    cacheCreationCostPerMillion: 0,
    isOverride: false,
    hasTimePricing: false,
    timeRules: [],
    cloudTimeRules: [],
    isUsed: false,
    contextTiers: [],
    aliases: [],
    userAliases: [],
    ...partial
  }
}

describe('matchesModelSearch', () => {
  it('空搜索全部通过', () => {
    const resolver = createModelResolver([pricing()])
    expect(matchesModelSearch(pricing(), '', resolver)).toBe(true)
  })

  it('模型名按 ASCII 大小写不敏感做部分匹配', () => {
    const model = pricing()
    const resolver = createModelResolver([model])
    expect(matchesModelSearch(model, 'claude', resolver)).toBe(true)
    expect(matchesModelSearch(model, 'OPUS', resolver)).toBe(true)
    expect(matchesModelSearch(model, 'Claude-Opus', resolver)).toBe(true)
    expect(matchesModelSearch(model, 'gpt', resolver)).toBe(false)
  })

  it('历史别名与用户别名参与匹配', () => {
    const model = pricing({ modelId: 'LongCat-2.5', aliases: ['long'], userAliases: ['龙猫'] })
    const resolver = createModelResolver([model])
    expect(matchesModelSearch(model, 'LONG', resolver)).toBe(true)
    expect(matchesModelSearch(model, '龙猫', resolver)).toBe(true)
    expect(matchesModelSearch(model, 'gpt', resolver)).toBe(false)
  })

  it('统一解析器命中别名正则时匹配 canonical 模型', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const model = pricing({ modelId: 'glm-5.3', aliasPatterns: ['^glm-5\\.3-flash$'] })
    const resolver = createModelResolver([model])
    expect(resolver.get('glm-5.3-flash')?.modelId).toBe('glm-5.3')
    expect(matchesModelSearch(model, 'glm-5.3-flash', resolver)).toBe(true)
    expect(matchesModelSearch(pricing({ modelId: 'other' }), 'glm-5.3-flash', resolver)).toBe(false)
    log.mockRestore()
  })

  it('解析器歧义返回 undefined 时不误匹配', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const conflict = createModelResolver([
      pricing({ modelId: 'a', aliases: ['dup'] }),
      pricing({ modelId: 'b', aliases: ['dup'] })
    ])
    expect(conflict.get('dup')).toBeUndefined()
    expect(matchesModelSearch(pricing({ modelId: 'a', aliases: ['dup'] }), 'dup', conflict)).toBe(true)
    expect(matchesModelSearch(pricing({ modelId: 'c' }), 'dup', conflict)).toBe(false)
    log.mockRestore()
  })
})
