import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { parseCloudPricing, fetchCloudPricing } from '../../../electron/main/services/cloud-pricing'
import sampleData from '../fixtures/cloud-pricing-sample.json'

// ---------------------------------------------------------------------------
// parseCloudPricing
// ---------------------------------------------------------------------------
describe('parseCloudPricing', () => {
  // 1. 完整有效数据
  it('正确解析完整的云端定价数据', () => {
    const result = parseCloudPricing(sampleData)

    expect(result.version).toBe(3)
    expect(result.updatedAt).toBe(1700000000)
    expect(result.currency).toBe('RMB')
    expect(result.models).toHaveLength(2)

    // 第一个模型 — Sonnet 4
    const sonnet = result.models[0]
    expect(sonnet.modelId).toBe('claude-sonnet-4-20250514')
    expect(sonnet.displayName).toBe('Claude Sonnet 4')
    expect(sonnet.inputCostPerMillion).toBe(21)
    expect(sonnet.outputCostPerMillion).toBe(105)
    expect(sonnet.cacheReadCostPerMillion).toBe(2.1)
    expect(sonnet.cacheCreationCostPerMillion).toBe(26.25)
    expect(sonnet.contextTiers).toHaveLength(2)
    expect(sonnet.contextTiers[0].threshold).toBe(10000)
    expect(sonnet.contextTiers[1].threshold).toBe(50000)
    expect(sonnet.timeRules).toHaveLength(1)
    expect(sonnet.timeRules[0].label).toBe('夜间折扣')

    // 第二个模型 — Haiku 4.5
    const haiku = result.models[1]
    expect(haiku.modelId).toBe('claude-haiku-4-5-20251001')
    expect(haiku.displayName).toBe('Claude Haiku 4.5')
    expect(haiku.inputCostPerMillion).toBe(4.2)
    expect(haiku.outputCostPerMillion).toBe(21)
    expect(haiku.contextTiers).toHaveLength(0)
    expect(haiku.timeRules).toHaveLength(0)
  })

  // 2. 缺失 contextTiers — 应默认为空数组
  it('缺失 contextTiers 时默认为空数组', () => {
    const data = {
      models: [{
        modelId: 'test-model',
        inputCostPerMillion: 10,
        outputCostPerMillion: 30
      }]
    }
    const result = parseCloudPricing(data)
    expect(result.models[0].contextTiers).toEqual([])
  })

  // 3. 缺失 timeRules — 应默认为空数组
  it('缺失 timeRules 时默认为空数组', () => {
    const data = {
      models: [{
        modelId: 'test-model',
        inputCostPerMillion: 10,
        outputCostPerMillion: 30
      }]
    }
    const result = parseCloudPricing(data)
    expect(result.models[0].timeRules).toEqual([])
  })

  // 4. 缺失 displayName — 应回退到 modelId
  it('缺失 displayName 时回退到 modelId', () => {
    const data = {
      models: [{
        modelId: 'my-model-id',
        inputCostPerMillion: 5,
        outputCostPerMillion: 15
      }]
    }
    const result = parseCloudPricing(data)
    expect(result.models[0].displayName).toBe('my-model-id')
  })

  // 5. 缺失 version — 默认为 1
  it('缺失 version 时默认为 1', () => {
    const data = {
      models: [{ modelId: 'm', inputCostPerMillion: 1, outputCostPerMillion: 2 }]
    }
    const result = parseCloudPricing(data)
    expect(result.version).toBe(1)
  })

  // 6. 缺失 currency — 默认为 "RMB"
  it('缺失 currency 时默认为 "RMB"', () => {
    const data = {
      models: [{ modelId: 'm', inputCostPerMillion: 1, outputCostPerMillion: 2 }]
    }
    const result = parseCloudPricing(data)
    expect(result.currency).toBe('RMB')
  })

  // 7. null 输入 — 抛错
  it('null 输入时抛出错误', () => {
    expect(() => parseCloudPricing(null)).toThrow('无效的云端定价数据格式')
  })

  // 8. 缺失 models — 抛错
  it('缺失 models 字段时抛出错误', () => {
    expect(() => parseCloudPricing({ version: 1 })).toThrow('无效的云端定价数据格式')
  })

  // 9. models 不是数组 — 抛错
  it('models 不是数组时抛出错误', () => {
    expect(() => parseCloudPricing({ models: 'not-array' })).toThrow('无效的云端定价数据格式')
  })

  // 10. 数值字段以字符串传入 — Number() 正确转换
  it('字符串形式的数值字段能正确转换为数字', () => {
    const data = {
      version: '5',
      updatedAt: '1700000000',
      models: [{
        modelId: 'str-num-model',
        inputCostPerMillion: '10.5',
        outputCostPerMillion: '52.5',
        cacheReadCostPerMillion: '1.05',
        cacheCreationCostPerMillion: '13.125'
      }]
    }
    const result = parseCloudPricing(data)
    expect(result.version).toBe(5)
    expect(result.updatedAt).toBe(1700000000)
    const model = result.models[0]
    expect(model.inputCostPerMillion).toBe(10.5)
    expect(model.outputCostPerMillion).toBe(52.5)
    expect(model.cacheReadCostPerMillion).toBe(1.05)
    expect(model.cacheCreationCostPerMillion).toBe(13.125)
  })

  // 11. timeRules 中嵌套的 contextTiers 正确解析
  it('timeRules 中嵌套的 contextTiers 能正确解析', () => {
    const data = {
      models: [{
        modelId: 'nested-model',
        inputCostPerMillion: 10,
        outputCostPerMillion: 30,
        cacheReadCostPerMillion: 1,
        cacheCreationCostPerMillion: 5,
        timeRules: [{
          label: '高峰时段',
          startTime: 1000,
          endTime: 2000,
          inputCostPerMillion: 20,
          outputCostPerMillion: 60,
          cacheReadCostPerMillion: 2,
          cacheCreationCostPerMillion: 10,
          contextTiers: [{
            threshold: 8000,
            inputCostPerMillion: 25,
            outputCostPerMillion: 75,
            cacheReadCostPerMillion: 2.5,
            cacheCreationCostPerMillion: 12.5
          }]
        }]
      }]
    }
    const result = parseCloudPricing(data)
    const rule = result.models[0].timeRules[0]
    expect(rule.label).toBe('高峰时段')
    expect(rule.startTime).toBe(1000)
    expect(rule.endTime).toBe(2000)
    expect(rule.inputCostPerMillion).toBe(20)
    expect(rule.contextTiers).toHaveLength(1)
    expect(rule.contextTiers[0].threshold).toBe(8000)
    expect(rule.contextTiers[0].inputCostPerMillion).toBe(25)
    expect(rule.contextTiers[0].outputCostPerMillion).toBe(75)
    expect(rule.contextTiers[0].cacheReadCostPerMillion).toBe(2.5)
    expect(rule.contextTiers[0].cacheCreationCostPerMillion).toBe(12.5)
  })

  // timeRules 中缺失 contextTiers — 默认为空数组
  it('timeRules 中缺失 contextTiers 时默认为空数组', () => {
    const data = {
      models: [{
        modelId: 'm',
        inputCostPerMillion: 1,
        outputCostPerMillion: 2,
        timeRules: [{
          startTime: 0,
          endTime: 100,
          inputCostPerMillion: 1,
          outputCostPerMillion: 2
        }]
      }]
    }
    const result = parseCloudPricing(data)
    expect(result.models[0].timeRules[0].contextTiers).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// fetchCloudPricing
// ---------------------------------------------------------------------------
describe('fetchCloudPricing', () => {
  const originalFetch = globalThis.fetch

  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
    globalThis.fetch = originalFetch
  })

  // 1. 成功响应
  it('成功获取并解析云端定价数据', async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(sampleData)
    })

    const result = await fetchCloudPricing('https://example.com/pricing.json')
    expect(result.version).toBe(3)
    expect(result.models).toHaveLength(2)
    expect(globalThis.fetch).toHaveBeenCalledWith(
      'https://example.com/pricing.json',
      expect.objectContaining({ signal: expect.any(AbortSignal) })
    )
  })

  // 2. HTTP 错误 — 抛错
  it('HTTP 错误时抛出包含状态码的错误', async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 503
    })

    await expect(fetchCloudPricing('https://example.com/pricing.json'))
      .rejects.toThrow('HTTP 503')
  })

  // 3. 网络异常 — 抛错
  it('网络异常时抛出错误', async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new TypeError('Failed to fetch'))

    await expect(fetchCloudPricing('https://example.com/pricing.json'))
      .rejects.toThrow('Failed to fetch')
  })
})
