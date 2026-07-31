// 定价类型定义

export interface PricingFamily {
  id: string
  label: string
}

/** 日内峰时窗口（当天分钟数，半开区间 [start, end)） */
export interface DailyWindow {
  startMinute: number
  endMinute: number
}

/** 扁平峰时价：挂在价格节点上，不再嵌套 contextTiers */
export interface DailySlot {
  label: string
  windows: DailyWindow[]
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
}

export interface ContextTier {
  id?: number                     // 时间规则档位行 ID（仅时间规则下的 tier 有）
  threshold: number               // 上下文大小边界（tokens）
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  dailySlots?: DailySlot[]
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
  dailySlots?: DailySlot[]
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
  dailySlots?: DailySlot[]
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
  dailySlots?: DailySlot[]
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
  userAliases: string[]
  noCacheSupport?: boolean
  family?: string
  dailySlots?: DailySlot[]
}

export interface TokenDimensions {
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}
