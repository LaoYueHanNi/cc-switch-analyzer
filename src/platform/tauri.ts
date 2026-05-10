import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { PlatformAdapter, DbResult, RefreshResult, FilterParams, PricingOverrideData, TimePricingRuleData, UpdateTimePricingRuleData } from './types'

function dateToStr(d: Date | null): string | undefined {
  if (!d) return undefined
  const yyyy = d.getFullYear()
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return `${yyyy}-${mm}-${dd}`
}

function toTauriParams(params: FilterParams): any {
  return {
    fromDate: dateToStr(params.fromDate),
    toDate: dateToStr(params.toDate),
    providerId: params.providerId || undefined,
    modelId: params.modelId || undefined
  }
}

export const platformAdapter: PlatformAdapter = {
  // 数据库 — Tauri 在前端开对话框，再传路径给后端
  async selectDatabase(): Promise<DbResult | null> {
    const selected = await open({
      title: '选择 CC-Switch 数据库文件',
      filters: [{ name: 'SQLite 数据库', extensions: ['db'] }],
      multiple: false
    })
    if (!selected) return null
    const filePath = typeof selected === 'string' ? selected : (selected as any).path
    return invoke<DbResult>('load_database', { filePath })
  },
  async autoLoadDatabase(): Promise<DbResult | null> {
    return invoke<DbResult | null>('auto_load_database')
  },
  async refreshDatabase(): Promise<RefreshResult> {
    return invoke<RefreshResult>('refresh_database')
  },
  // 查询 — 日期转字符串给 Rust
  async querySummary(params: FilterParams) {
    return invoke('query_summary', { params: toTauriParams(params) })
  },
  async queryByModel(params: FilterParams) {
    return invoke('query_by_model', { params: toTauriParams(params) })
  },
  async queryByProvider(params: FilterParams) {
    return invoke('query_by_provider', { params: toTauriParams(params) })
  },
  async queryPrecompute(params: FilterParams) {
    return invoke('query_precompute', { params: toTauriParams(params) })
  },
  async queryRealtime() {
    return invoke('query_realtime')
  },
  async queryRealtimeLogs(since?: number) {
    return invoke('query_realtime_logs', { since: since ?? null })
  },
  async querySessionsWithCost(params: FilterParams) {
    return invoke('query_sessions_with_cost', { params: toTauriParams(params) })
  },
  // 定价
  async getAllPricing() {
    return invoke('get_all_pricing')
  },
  async setPricingOverride(data: PricingOverrideData) {
    return invoke('set_pricing_override', {
      modelId: data.modelId, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async removePricingOverride(modelId: string) {
    return invoke('remove_pricing_override', { modelId })
  },
  async addTimePricingRule(data: TimePricingRuleData) {
    return invoke('add_time_pricing_rule', {
      modelId: data.modelId, startTime: data.startTime, endTime: data.endTime,
      input: data.input, output: data.output, cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation, label: data.label
    })
  },
  async updateTimePricingRule(data: UpdateTimePricingRuleData) {
    return invoke('update_time_pricing_rule', {
      id: data.id, startTime: data.startTime, endTime: data.endTime,
      input: data.input, output: data.output, cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation, label: data.label
    })
  },
  async deleteTimePricingRule(data: { modelId: string; startTime: number; endTime: number; id: number }) {
    return invoke('delete_time_pricing_rule', {
      modelId: data.modelId, startTime: data.startTime, endTime: data.endTime, id: data.id
    })
  },
  async refreshPricing() {
    return invoke('refresh_pricing')
  },
  async saveOverrideContextTier(data: { modelId: string; threshold: number; input: number; output: number; cacheRead: number; cacheCreation: number }) {
    return invoke('save_override_context_tier', {
      modelId: data.modelId, threshold: data.threshold, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async deleteOverrideContextTier(data: { modelId: string; threshold: number }) {
    return invoke('delete_override_context_tier', {
      modelId: data.modelId, threshold: data.threshold
    })
  },
  async saveTimeRuleContextTier(data: { modelId: string; startTime: number; endTime: number; threshold: number; input: number; output: number; cacheRead: number; cacheCreation: number }) {
    return invoke('save_time_rule_context_tier', {
      modelId: data.modelId, startTime: data.startTime, endTime: data.endTime,
      threshold: data.threshold, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async updateTimeRuleContextTier(data: { id: number; input: number; output: number; cacheRead: number; cacheCreation: number }) {
    return invoke('update_time_rule_context_tier', {
      id: data.id, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async deleteTimeRuleContextTier(id: number) {
    return invoke('delete_time_rule_context_tier', { id })
  },
  async fetchCloudPricing() {
    return invoke('fetch_cloud_pricing')
  },
  async addUserAlias(modelId: string, alias: string) {
    return invoke('add_user_alias', { modelId, alias })
  },
  async removeUserAlias(modelId: string, alias: string) {
    return invoke('remove_user_alias', { modelId, alias })
  },
  async getSessionTitles(sessionIds: string[]) {
    return invoke<Record<string, { title: string; project: string }>>('get_session_titles', { sessionIds })
  }
}
