// 定价类型定义

export interface ModelPricing {
  modelId: string
  displayName: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
}

export interface MergedPricing extends ModelPricing {
  isOverride: boolean
}

export interface PricingOverride {
  modelId: string
  inputCostPerMillion: number     // RMB
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  updatedAt: number
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
}

export interface PricingData {
  modelId: string
  displayName: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  isOverride: boolean
  hasTimePricing: boolean
  timeRules: TimePricingRule[]
  isUsed: boolean
}

export interface TokenDimensions {
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}
