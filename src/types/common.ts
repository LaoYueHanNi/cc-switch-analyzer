// 通用类型定义

import type { DailyTrendRow } from './database'

export interface PrecomputedResult {
  modelCosts: Map<string, number>
  modelCostBreakdown: Map<string, [number, number, number, number]>
  providerCosts: Map<string, number>
  dayCostMap: Map<string, number>
  dayRequestsMap: Map<string, number>
  dailyByModel: Map<string, DailyTrendRow[]>
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
