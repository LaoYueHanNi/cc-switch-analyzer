import type { SummaryData, ModelBreakdown, ProviderBreakdown, RealtimeBucket, RealtimeRequestLog } from '@/types/database'
import type { PrecomputeQueryResult, SessionWithCost } from '@/types/common'
import type { PricingData } from '@/types/pricing'

export interface DbResult {
  path: string
  recordCount: number
  dateRange: { min: number; max: number }
  providers: { id: string; name: string }[]
  models: string[]
}

export interface RefreshResult {
  hasNew: boolean
  recordCount: number | null
}

export interface FilterParams {
  fromDate: Date | null
  toDate: Date | null
  providerId: string
  modelId: string
}

export interface PricingOverrideData {
  modelId: string
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}

export interface TimePricingRuleData {
  modelId: string
  startTime: number
  endTime: number
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
  label: string
}

export interface UpdateTimePricingRuleData {
  id: number
  startTime: number
  endTime: number
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
  label: string
}

export interface ContextTierData {
  modelId: string
  threshold: number
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}

export interface TimeRuleContextTierData {
  modelId: string
  startTime: number
  endTime: number
  threshold: number
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}

export interface DeleteTimePricingRuleData {
  id: number
}

export interface PlatformAdapter {
  // 数据库
  selectDatabase(): Promise<DbResult | null>
  autoLoadDatabase(): Promise<DbResult | null>
  refreshDatabase(): Promise<RefreshResult>
  // 查询
  querySummary(params: FilterParams): Promise<SummaryData>
  queryByModel(params: FilterParams): Promise<ModelBreakdown[]>
  queryByProvider(params: FilterParams): Promise<ProviderBreakdown[]>
  queryPrecompute(params: FilterParams): Promise<PrecomputeQueryResult>
  queryRealtime(): Promise<RealtimeBucket[]>
  queryRealtimeLogs(since?: number): Promise<RealtimeRequestLog[]>
  querySessionsWithCost(params: FilterParams): Promise<SessionWithCost[]>
  // 定价
  getAllPricing(): Promise<PricingData[]>
  setPricingOverride(data: PricingOverrideData): Promise<void>
  removePricingOverride(modelId: string): Promise<void>
  addTimePricingRule(data: TimePricingRuleData): Promise<void>
  updateTimePricingRule(data: UpdateTimePricingRuleData): Promise<void>
  deleteTimePricingRule(data: DeleteTimePricingRuleData): Promise<void>
  refreshPricing(): Promise<void>
  // 上下文定价档位
  saveOverrideContextTier(data: ContextTierData): Promise<void>
  deleteOverrideContextTier(data: { modelId: string; threshold: number }): Promise<void>
  saveTimeRuleContextTier(data: TimeRuleContextTierData): Promise<void>
  updateTimeRuleContextTier(data: { id: number; input: number; output: number; cacheRead: number; cacheCreation: number }): Promise<void>
  deleteTimeRuleContextTier(id: number): Promise<void>
  // 云端定价
  fetchCloudPricing(): Promise<void>
  // 用户别名
  addUserAlias(modelId: string, alias: string): Promise<void>
  removeUserAlias(modelId: string, alias: string): Promise<void>
  getSessionTitles(sessionIds: string[]): Promise<Record<string, { title: string; project: string }>>
}
