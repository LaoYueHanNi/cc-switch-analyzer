// 数据库类型定义

export interface FilterParams {
  fromDate: Date | null
  toDate: Date | null
  providerId: string  // 空字符串 = 全部
  modelId: string     // 空字符串 = 全部
}

export interface Provider {
  id: string
  appType: string
  name: string
}

export interface SummaryData {
  totalRequests: number
  successCount: number
  totalInput: number
  totalOutput: number
  totalCacheRead: number
  totalCacheCreation: number
  avgLatency: number
}

export interface ModelBreakdown {
  model: string
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface ProviderBreakdown {
  providerName: string
  providerId: string
  requests: number
  successes: number
  successRate: number
  avgLatency: number
}

export interface ProviderModelToken {
  providerId: string
  model: string
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface DailyTrendRow {
  day: string
  model: string
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
  avgLatency: number
}

export interface RealtimeBucket {
  bucket: number
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface RealtimeRequestLog {
  sessionId: string
  model: string
  providerId: string
  createdAt: number
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheCreationTokens: number
  latencyMs: number
  inputCost: number
  outputCost: number
  cacheReadCost: number
  cacheCreationCost: number
  totalCost: number
  contextTierThreshold?: number
}
