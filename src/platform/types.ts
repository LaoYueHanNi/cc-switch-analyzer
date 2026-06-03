import type { SummaryData, ModelBreakdown, ProviderBreakdown, RealtimeBucket, RealtimeRequestLog, DailyTrendRow } from '@/types/database'
import type { PrecomputeQueryResult, SessionWithCost, SessionModelCostEntry } from '@/types/common'
import type { PricingData } from '@/types/pricing'
import type { Task, TaskSession, TaskSessionInput, TaskWithStats, TaskDetail } from '@/types/task'

export interface ProjectGroupStats {
  projectDir: string
  displayName: string
  sessionCount: number
  totalCost: number
  totalTokens: number
  firstAt: number
  lastAt: number
  sessionIds: string[]
  sourceTypes?: string[]
}

export interface ProjectSessionDetail {
  sessionId: string
  requestCount: number
  totalTokens: number
  totalCost: number
  startTime: number
  endTime: number
  durationSec: number
  maxContextWidth: number
  cacheHitRate: number
  timestamps: number[]
  modelBreakdown: SessionModelCostEntry[]
  title?: string
  projectDir?: string
  sourcePath?: string
  sourceType?: string
}

export interface DbResult {
  path: string
  recordCount: number
  dateRange: { min: number; max: number }
  providers: { id: string; name: string }[]
  models: string[]
}

export interface SourceInfo {
  id: string
  path: string
  dbType: string
  recordCount: number
  enabled: boolean
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

export interface UpdateInfo {
  version: string
  currentVersion: string
  date?: string
  body?: string
}

export interface PlatformAdapter {
  // 数据库
  selectDatabase(): Promise<DbResult | null>
  autoLoadDatabase(): Promise<SourceInfo[]>
  addDatabase(filePath: string): Promise<SourceInfo[]>
  removeDatabase(sourceId: string): Promise<SourceInfo[]>
  toggleDatabase(sourceId: string): Promise<SourceInfo[]>
  listDatabases(): Promise<SourceInfo[]>
  pickDatabaseFile(defaultPath?: string): Promise<string | null>
  refreshDatabase(): Promise<RefreshResult>
  getFilterOptions(): Promise<{ providers: { id: string; name: string }[]; models: string[]; dateRange: { min: number; max: number } }>
  // 查询
  querySummary(params: FilterParams): Promise<SummaryData>
  queryByModel(params: FilterParams): Promise<ModelBreakdown[]>
  queryByProvider(params: FilterParams): Promise<ProviderBreakdown[]>
  queryPrecompute(params: FilterParams): Promise<PrecomputeQueryResult>
  queryHourlyTrend(params: FilterParams): Promise<DailyTrendRow[]>
  queryRealtime(): Promise<RealtimeBucket[]>
  queryRealtimeLogs(since?: number): Promise<RealtimeRequestLog[]>
  querySessionsWithCost(params: FilterParams, project?: string): Promise<{ sessions: SessionWithCost[]; availableProjects: string[] }>
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
  // 会话管理
  querySessionProjectGroups(params: FilterParams): Promise<ProjectGroupStats[]>
  queryProjectSessionDetails(params: FilterParams, sessionIds: string[]): Promise<ProjectSessionDetail[]>
  openClaudeTerminal(projectDir: string): Promise<void>
  openOpenCodeTerminal(projectDir: string): Promise<void>
  resumeClaudeSession(sessionId: string, projectDir?: string): Promise<void>
  deleteClaudeSession(sessionId: string): Promise<boolean>
  getCcswitchProviders(dbPath: string): Promise<{ id: string; name: string; hasEnv: boolean }[]>
  openClaudeTerminalWithProvider(projectDir: string, providerId: string, dbPath: string): Promise<void>
  resumeClaudeSessionWithProvider(sessionId: string, providerId: string, dbPath: string, projectDir?: string): Promise<void>
  resumeOpenCodeSession(sessionId: string, projectDir?: string): Promise<void>
  openCodexTerminal(projectDir: string): Promise<void>
  resumeCodexSession(sessionId: string, projectDir?: string): Promise<void>
  // 任务管理
  listTasks(): Promise<TaskWithStats[]>
  getTaskDetail(taskId: number): Promise<TaskDetail>
  createTask(title: string, description: string, status: string): Promise<number>
  updateTask(taskId: number, title: string, description: string, status: string): Promise<void>
  deleteTask(taskId: number): Promise<void>
  addSessionsToTask(taskId: number, sessions: TaskSessionInput[]): Promise<void>
  getTaskSessionDetail(taskId: number, sessionId: string): Promise<ProjectSessionDetail>
  openTaskAgent(agentSource: string, projectDir: string, providerId?: string, dbPath?: string): Promise<void>
  // 更新
  checkForUpdate(proxy?: string): Promise<UpdateInfo | null>
  downloadAndInstall(onProgress?: (downloaded: number) => void, proxy?: string): Promise<void>
}
