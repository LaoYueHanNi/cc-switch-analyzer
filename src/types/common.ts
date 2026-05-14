// 通用类型定义

import type { DailyTrendRow, SummaryData, ModelBreakdown, ProviderBreakdown } from './database'

export interface ContextTierCost {
  threshold: number
  cost: number
  tokens: number
}

export interface PrecomputedResult {
  modelCosts: Record<string, number>
  modelCostBreakdown: Record<string, number[]>
  providerCosts: Record<string, number>
  dayCostMap: Record<string, number>
  dayRequestsMap: Record<string, number>
  dayInputTokens: Record<string, number>
  dayOutputTokens: Record<string, number>
  dayLatencySum: Record<string, number>
  dayLatencyCount: Record<string, number>
  dailyByModel: Record<string, DailyTrendRow[]>
  modelContextTierCosts?: Record<string, ContextTierCost[]>
  unpricedModels?: string[]
}

export interface PrecomputeQueryResult {
  summary: SummaryData
  modelBreakdown: ModelBreakdown[]
  providerBreakdown: ProviderBreakdown[]
  precomputed: PrecomputedResult
}

export interface SessionStat {
  sessionId: string
  requestCount: number
  totalTokens: number
  maxContextWidth: number
  startTime: number
  endTime: number
  cacheHitRate: number
  modelBreakdown: SessionModelToken[]
  timestamps: number[]
  totalCost: number
  durationSec: number
}

export interface SessionModelToken {
  sessionId: string
  model: string
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface SessionRequestToken {
  sessionId: string
  model: string
  createdAt: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface SessionModelCostEntry {
  sessionId: string
  model: string
  cost: number
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheCreationTokens: number
  inputCost: number
  outputCost: number
  cacheReadCost: number
  cacheCreationCost: number
  contextTierCosts?: ContextTierCost[]
}

export interface SessionWithCost {
  sessionId: string
  requestCount: number
  totalTokens: number
  maxContextWidth: number
  startTime: number
  endTime: number
  cacheHitRate: number
  totalCost: number
  durationSec: number
  timestamps: number[]
  modelBreakdown: SessionModelCostEntry[]
  sources: string[]
}
