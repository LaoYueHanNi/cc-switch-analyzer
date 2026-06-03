import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { open } from '@tauri-apps/plugin-dialog'
import { check } from '@tauri-apps/plugin-updater'
import type { PlatformAdapter, DbResult, SourceInfo, RefreshResult, FilterParams, PricingOverrideData, TimePricingRuleData, UpdateTimePricingRuleData, ProjectGroupStats, ProjectSessionDetail, UpdateInfo, OpenTaskSessionsResult } from './types'
import type { SummaryData, ModelBreakdown, ProviderBreakdown, RealtimeBucket, RealtimeRequestLog, DailyTrendRow } from '@/types/database'
import type { PrecomputeQueryResult, SessionWithCost } from '@/types/common'
import type { PricingData } from '@/types/pricing'
import type { TaskWithStats, TaskSessionInput } from '@/types/task'

interface TauriFilterParams {
  fromEpoch?: number
  toEpoch?: number
  tzOffset: number
  providerId?: string
  modelId?: string
}

function localDateToEpoch(d: Date): number {
  return Math.floor(new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() / 1000)
}

function localDateToEpochEndExclusive(d: Date): number {
  return Math.floor(new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime() / 1000)
}

function toTauriParams(params: FilterParams): TauriFilterParams {
  return {
    fromEpoch: params.fromDate ? localDateToEpoch(params.fromDate) : undefined,
    toEpoch: params.toDate ? localDateToEpochEndExclusive(params.toDate) : undefined,
    tzOffset: -(new Date().getTimezoneOffset() / 60),
    providerId: params.providerId || undefined,
    modelId: params.modelId || undefined
  }
}

export const platformAdapter: PlatformAdapter = {
  // 数据库 — Tauri 在前端开对话框，再传路径给后端
  async selectDatabase(): Promise<DbResult | null> {
    const selected = await open({
      title: '选择数据库文件',
      filters: [{ name: 'SQLite 数据库', extensions: ['db', 'sqlite', 'sqlite3'] }],
      multiple: false
    })
    if (!selected) return null
    const filePath = typeof selected === 'string' ? selected : (selected as any).path
    const sources = await invoke<SourceInfo[]>('load_database', { filePath })
    // 兼容旧接口：返回第一个 source 的 DbResult
    if (sources.length === 0) return null
    const info = sources[0]
    return { path: info.path, recordCount: info.recordCount, dateRange: { min: 0, max: 0 }, providers: [], models: [] }
  },
  async autoLoadDatabase(): Promise<SourceInfo[]> {
    return invoke<SourceInfo[]>('auto_load_database')
  },
  async addDatabase(filePath: string): Promise<SourceInfo[]> {
    return invoke<SourceInfo[]>('add_database', { filePath })
  },
  async removeDatabase(sourceId: string): Promise<SourceInfo[]> {
    return invoke<SourceInfo[]>('remove_database', { sourceId })
  },
  async toggleDatabase(sourceId: string): Promise<SourceInfo[]> {
    return invoke<SourceInfo[]>('toggle_database', { sourceId })
  },
  async listDatabases(): Promise<SourceInfo[]> {
    return invoke<SourceInfo[]>('list_databases')
  },
  async pickDatabaseFile(defaultPath?: string): Promise<string | null> {
    const selected = await open({
      title: '选择数据库文件',
      filters: [{ name: 'SQLite 数据库', extensions: ['db', 'sqlite', 'sqlite3'] }],
      multiple: false,
      defaultPath
    })
    if (!selected) return null
    return typeof selected === 'string' ? selected : (selected as any).path
  },
  async refreshDatabase(): Promise<RefreshResult> {
    return invoke<RefreshResult>('refresh_database')
  },
  async getFilterOptions() {
    return invoke<{ providers: { id: string; name: string }[]; models: string[]; dateRange: { min: number; max: number } }>('get_filter_options')
  },
  // 查询 — 日期转字符串给 Rust
  async querySummary(params: FilterParams): Promise<SummaryData> {
    return invoke<SummaryData>('query_summary', { params: toTauriParams(params) })
  },
  async queryByModel(params: FilterParams): Promise<ModelBreakdown[]> {
    return invoke<ModelBreakdown[]>('query_by_model', { params: toTauriParams(params) })
  },
  async queryByProvider(params: FilterParams): Promise<ProviderBreakdown[]> {
    return invoke<ProviderBreakdown[]>('query_by_provider', { params: toTauriParams(params) })
  },
  async queryPrecompute(params: FilterParams): Promise<PrecomputeQueryResult> {
    return invoke<PrecomputeQueryResult>('query_precompute', { params: toTauriParams(params) })
  },
  async queryHourlyTrend(params: FilterParams): Promise<DailyTrendRow[]> {
    return invoke<DailyTrendRow[]>('query_hourly_trend', { params: toTauriParams(params) })
  },
  async queryRealtime(): Promise<RealtimeBucket[]> {
    return invoke<RealtimeBucket[]>('query_realtime')
  },
  async queryRealtimeLogs(since?: number): Promise<RealtimeRequestLog[]> {
    return invoke<RealtimeRequestLog[]>('query_realtime_logs', { since: since ?? null })
  },
  async querySessionsWithCost(params: FilterParams, project?: string): Promise<{ sessions: SessionWithCost[]; availableProjects: string[] }> {
    return invoke<{ sessions: SessionWithCost[]; availableProjects: string[] }>('query_sessions_with_cost', { params: toTauriParams(params), project: project || null })
  },
  // 定价
  async getAllPricing(): Promise<PricingData[]> {
    return invoke<PricingData[]>('get_all_pricing')
  },
  async setPricingOverride(data: PricingOverrideData): Promise<void> {
    return invoke<void>('set_pricing_override', {
      modelId: data.modelId, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async removePricingOverride(modelId: string): Promise<void> {
    return invoke<void>('remove_pricing_override', { modelId })
  },
  async addTimePricingRule(data: TimePricingRuleData): Promise<void> {
    return invoke<void>('add_time_pricing_rule', {
      modelId: data.modelId, startTime: data.startTime, endTime: data.endTime,
      input: data.input, output: data.output, cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation, label: data.label
    })
  },
  async updateTimePricingRule(data: UpdateTimePricingRuleData): Promise<void> {
    return invoke<void>('update_time_pricing_rule', {
      id: data.id, startTime: data.startTime, endTime: data.endTime,
      input: data.input, output: data.output, cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation, label: data.label
    })
  },
  async deleteTimePricingRule(data: { id: number }): Promise<void> {
    return invoke<void>('delete_time_pricing_rule', {
      id: data.id
    })
  },
  async refreshPricing(): Promise<void> {
    return invoke<void>('refresh_pricing')
  },
  async saveOverrideContextTier(data: { modelId: string; threshold: number; input: number; output: number; cacheRead: number; cacheCreation: number }): Promise<void> {
    return invoke<void>('save_override_context_tier', {
      modelId: data.modelId, threshold: data.threshold, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async deleteOverrideContextTier(data: { modelId: string; threshold: number }): Promise<void> {
    return invoke<void>('delete_override_context_tier', {
      modelId: data.modelId, threshold: data.threshold
    })
  },
  async saveTimeRuleContextTier(data: { modelId: string; startTime: number; endTime: number; threshold: number; input: number; output: number; cacheRead: number; cacheCreation: number }): Promise<void> {
    return invoke<void>('save_time_rule_context_tier', {
      modelId: data.modelId, startTime: data.startTime, endTime: data.endTime,
      threshold: data.threshold, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async updateTimeRuleContextTier(data: { id: number; input: number; output: number; cacheRead: number; cacheCreation: number }): Promise<void> {
    return invoke<void>('update_time_rule_context_tier', {
      id: data.id, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async deleteTimeRuleContextTier(id: number): Promise<void> {
    return invoke<void>('delete_time_rule_context_tier', { id })
  },
  async fetchCloudPricing(): Promise<void> {
    return invoke<void>('fetch_cloud_pricing')
  },
  async addUserAlias(modelId: string, alias: string): Promise<void> {
    return invoke<void>('add_user_alias', { modelId, alias })
  },
  async removeUserAlias(modelId: string, alias: string): Promise<void> {
    return invoke<void>('remove_user_alias', { modelId, alias })
  },
  async getSessionTitles(sessionIds: string[]) {
    return invoke<Record<string, { title: string; project: string }>>('get_session_titles', { sessionIds })
  },
  // 会话管理
  async querySessionProjectGroups(params: FilterParams): Promise<ProjectGroupStats[]> {
    return invoke<ProjectGroupStats[]>('query_session_project_groups', { params: toTauriParams(params) })
  },
  async queryProjectSessionDetails(params: FilterParams, sessionIds: string[]): Promise<ProjectSessionDetail[]> {
    return invoke<ProjectSessionDetail[]>('query_project_session_details', { params: toTauriParams(params), sessionIds })
  },
  async openClaudeTerminal(projectDir: string): Promise<void> {
    return invoke<void>('open_claude_terminal', { projectDir })
  },
  async openOpenCodeTerminal(projectDir: string): Promise<void> {
    return invoke<void>('open_opencode_terminal', { projectDir })
  },
  async resumeClaudeSession(sessionId: string, projectDir?: string): Promise<void> {
    return invoke<void>('resume_claude_session', { sessionId, projectDir: projectDir || null })
  },
  async deleteClaudeSession(sessionId: string): Promise<boolean> {
    return invoke<boolean>('delete_claude_session', { sessionId })
  },
  async getCcswitchProviders(dbPath: string): Promise<{ id: string; name: string; hasEnv: boolean }[]> {
    return invoke<{ id: string; name: string; hasEnv: boolean }[]>('get_ccswitch_providers', { dbPath })
  },
  async openClaudeTerminalWithProvider(projectDir: string, providerId: string, dbPath: string): Promise<void> {
    return invoke<void>('open_claude_terminal_with_provider', { projectDir, providerId, dbPath })
  },
  async resumeClaudeSessionWithProvider(sessionId: string, providerId: string, dbPath: string, projectDir?: string): Promise<void> {
    return invoke<void>('resume_claude_session_with_provider', { sessionId, projectDir: projectDir || null, providerId, dbPath })
  },
  async resumeOpenCodeSession(sessionId: string, projectDir?: string): Promise<void> {
    return invoke<void>('resume_opencode_session', { sessionId, projectDir: projectDir || null })
  },
  async openCodexTerminal(projectDir: string): Promise<void> {
    return invoke<void>('open_codex_terminal', { projectDir })
  },
  async resumeCodexSession(sessionId: string, projectDir?: string): Promise<void> {
    return invoke<void>('resume_codex_session', { sessionId, projectDir: projectDir || null })
  },
  // 任务管理
  async listTasks(): Promise<TaskWithStats[]> {
    return invoke<TaskWithStats[]>('list_tasks')
  },
  async getTaskDetail(taskId: number): Promise<TaskDetail> {
    return invoke<TaskDetail>('get_task_detail', { taskId })
  },
  async createTask(title: string, description: string, status: string): Promise<number> {
    return invoke<number>('create_task', { title, description, status })
  },
  async updateTask(taskId: number, title: string, description: string, status: string): Promise<void> {
    return invoke<void>('update_task', { taskId, title, description, status })
  },
  async deleteTask(taskId: number): Promise<void> {
    return invoke<void>('delete_task', { taskId })
  },
  async addSessionsToTask(taskId: number, sessions: TaskSessionInput[]): Promise<void> {
    return invoke<void>('add_sessions_to_task', { taskId, sessions })
  },
  async getTaskSessionDetail(taskId: number, sessionId: string): Promise<ProjectSessionDetail> {
    return invoke<ProjectSessionDetail>('get_task_session_detail', { taskId, sessionId })
  },
  async openTaskAgent(agentSource: string, projectDir: string, providerId?: string, dbPath?: string): Promise<void> {
    return invoke<void>('open_task_agent', {
      agentSource,
      projectDir,
      providerId: providerId || null,
      dbPath: dbPath || null
    })
  },
  async openTaskSessions(taskId: number): Promise<OpenTaskSessionsResult> {
    return invoke<OpenTaskSessionsResult>('open_task_sessions', { taskId })
  },
  async checkForUpdate(proxy?: string): Promise<UpdateInfo | null> {
    const opts = proxy ? { proxy } : undefined
    const update = await check(opts)
    if (!update) return null
    const currentVersion = update.currentVersion || await getVersion()
    return {
      version: update.version,
      currentVersion,
      date: update.date ?? undefined,
      body: update.body ?? undefined,
    }
  },
  async downloadAndInstall(onProgress?: (downloaded: number) => void, proxy?: string): Promise<void> {
    const opts = proxy ? { proxy } : undefined
    const update = await check(opts)
    if (!update) throw new Error('没有可用的更新')
    console.log('[updater] downloading version', update.version)
    let downloaded = 0
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          console.log('[updater] download started, contentLength:', event.data.contentLength)
          break
        case 'Progress':
          downloaded += event.data.chunkLength
          onProgress?.(downloaded)
          break
        case 'Finished':
          console.log('[updater] download finished, total:', downloaded)
          break
      }
    })
    console.log('[updater] install complete')
  }
}
