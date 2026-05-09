// 定价类型定义

export interface ContextTier {
  id?: number                     // 时间规则档位行 ID（仅时间规则下的 tier 有）
  threshold: number               // 上下文大小边界（tokens）
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
}

export interface ModelPricing {
  modelId: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  aliases: string[]
}

export interface MergedPricing {
  modelId: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  isOverride: boolean
}

export interface PricingOverride {
  modelId: string
  inputCostPerMillion: number     // RMB
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  updatedAt: number
  contextTiers: ContextTier[]
}

export interface TimePricingRule {
  id: number
  modelId: string
  startTime: number               // Unix 秒
  endTime: number                 // Unix 秒
  inputCostPerMillion: number     // RMB
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  label: string
  contextTiers: ContextTier[]
}

export interface CloudPricingTimeRule {
  label: string
  startTime: number
  endTime: number
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  contextTiers: ContextTier[]
}

export interface PricingData {
  modelId: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  isOverride: boolean
  hasTimePricing: boolean
  timeRules: TimePricingRule[]
  cloudTimeRules: CloudPricingTimeRule[]
  isUsed: boolean
  contextTiers: ContextTier[]
  aliases: string[]
}

export interface TokenDimensions {
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}
