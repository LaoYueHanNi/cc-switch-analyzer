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

export interface PlatformAdapter {
  // 数据库
  selectDatabase(): Promise<DbResult | null>
  autoLoadDatabase(): Promise<DbResult | null>
  refreshDatabase(): Promise<RefreshResult>
  // 查询
  querySummary(params: FilterParams): Promise<any>
  queryByModel(params: FilterParams): Promise<any>
  queryByProvider(params: FilterParams): Promise<any>
  queryPrecompute(params: FilterParams): Promise<any>
  queryRealtime(): Promise<any>
  queryRealtimeLogs(): Promise<any>
  queryCacheWindows(modelId: string): Promise<any[]>
  querySessionsWithCost(params: FilterParams): Promise<any[]>
  // 定价
  getExchangeRate(): Promise<number>
  setExchangeRate(rate: number): Promise<void>
  getAllPricing(): Promise<any[]>
  setPricingOverride(data: PricingOverrideData): Promise<void>
  removePricingOverride(modelId: string): Promise<void>
  addTimePricingRule(data: TimePricingRuleData): Promise<any>
  updateTimePricingRule(data: UpdateTimePricingRuleData): Promise<void>
  deleteTimePricingRule(id: number): Promise<void>
  refreshPricing(): Promise<void>
}
