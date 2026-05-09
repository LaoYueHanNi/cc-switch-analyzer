import { CLOUD_PRICING_URL } from '../utils/constants'
import type { CloudPricingData } from './app-db'

const TIMEOUT_MS = 5000

/// 从云端拉取定价 JSON 并解析
export async function fetchCloudPricing(url: string = CLOUD_PRICING_URL): Promise<CloudPricingData> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), TIMEOUT_MS)

  try {
    const response = await fetch(url, { signal: controller.signal })
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`)
    }
    const json = await response.json()
    return parseCloudPricing(json)
  } finally {
    clearTimeout(timer)
  }
}

/// 解析云端定价 JSON
export function parseCloudPricing(data: any): CloudPricingData {
  if (!data || !Array.isArray(data.models)) {
    throw new Error('无效的云端定价数据格式')
  }
  return {
    version: Number(data.version) || 1,
    updatedAt: Number(data.updatedAt) || 0,
    currency: String(data.currency || 'RMB'),
    models: data.models.map((m: any) => ({
      modelId: String(m.modelId),
      inputCostPerMillion: Number(m.inputCostPerMillion),
      outputCostPerMillion: Number(m.outputCostPerMillion),
      cacheReadCostPerMillion: Number(m.cacheReadCostPerMillion),
      cacheCreationCostPerMillion: Number(m.cacheCreationCostPerMillion),
      aliases: Array.isArray(m.aliases) ? m.aliases.map((a: any) => String(a)) : [],
      contextTiers: Array.isArray(m.contextTiers)
        ? m.contextTiers.map((t: any) => ({
            threshold: Number(t.threshold),
            inputCostPerMillion: Number(t.inputCostPerMillion),
            outputCostPerMillion: Number(t.outputCostPerMillion),
            cacheReadCostPerMillion: Number(t.cacheReadCostPerMillion),
            cacheCreationCostPerMillion: Number(t.cacheCreationCostPerMillion)
          }))
        : [],
      timeRules: Array.isArray(m.timeRules)
        ? m.timeRules.map((r: any) => ({
            label: String(r.label || ''),
            startTime: Number(r.startTime),
            endTime: Number(r.endTime),
            inputCostPerMillion: Number(r.inputCostPerMillion),
            outputCostPerMillion: Number(r.outputCostPerMillion),
            cacheReadCostPerMillion: Number(r.cacheReadCostPerMillion),
            cacheCreationCostPerMillion: Number(r.cacheCreationCostPerMillion),
            contextTiers: Array.isArray(r.contextTiers)
              ? r.contextTiers.map((t: any) => ({
                  threshold: Number(t.threshold),
                  inputCostPerMillion: Number(t.inputCostPerMillion),
                  outputCostPerMillion: Number(t.outputCostPerMillion),
                  cacheReadCostPerMillion: Number(t.cacheReadCostPerMillion),
                  cacheCreationCostPerMillion: Number(t.cacheCreationCostPerMillion)
                }))
              : []
          }))
        : []
    }))
  }
}
