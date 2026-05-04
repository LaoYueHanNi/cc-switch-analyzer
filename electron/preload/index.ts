import { contextBridge, ipcRenderer } from 'electron'

// 暴露给渲染进程的类型安全 API
const api = {
  // 数据库操作
  selectDatabase: () => ipcRenderer.invoke('db:select-file'),
  autoLoadDatabase: () => ipcRenderer.invoke('db:auto-load'),
  loadDatabase: (filePath: string) => ipcRenderer.invoke('db:load', filePath),
  refreshDatabase: () => ipcRenderer.invoke('db:refresh'),
  getFilterOptions: () => ipcRenderer.invoke('db:get-filter-options'),

  // 数据查询
  querySummary: (params: any) => ipcRenderer.invoke('query:summary', params),
  queryByModel: (params: any) => ipcRenderer.invoke('query:by-model', params),
  queryByProvider: (params: any) => ipcRenderer.invoke('query:by-provider', params),
  queryProviderModelTokens: (params: any) => ipcRenderer.invoke('query:provider-model-tokens', params),
  queryDailyTrend: (params: any) => ipcRenderer.invoke('query:daily-trend', params),
  queryCacheDurations: (params: any) => ipcRenderer.invoke('query:cache-durations', params),
  queryCacheWindows: (modelId: string) => ipcRenderer.invoke('query:cache-windows', modelId),
  querySessions: (params: any) => ipcRenderer.invoke('query:sessions', params),
  querySessionModelTokens: (params: any) => ipcRenderer.invoke('query:session-model-tokens', params),
  querySessionRequestTokens: (params: any) => ipcRenderer.invoke('query:session-request-tokens', params),
  querySessionTimestamps: (sessionIds: string[]) => ipcRenderer.invoke('query:session-timestamps', sessionIds),
  queryRealtime: () => ipcRenderer.invoke('query:realtime'),
  queryRealtimeLogs: () => ipcRenderer.invoke('query:realtime-logs'),
  queryPrecompute: (params: any) => ipcRenderer.invoke('query:precompute', params),
  querySessionsWithCost: (params: any) => ipcRenderer.invoke('query:sessions-with-cost', params),

  // 定价操作
  getExchangeRate: () => ipcRenderer.invoke('pricing:get-exchange-rate'),
  setExchangeRate: (rate: number) => ipcRenderer.invoke('pricing:set-exchange-rate', rate),
  getAllPricing: () => ipcRenderer.invoke('pricing:get-all'),
  getPricingOverrides: () => ipcRenderer.invoke('pricing:get-overrides'),
  setPricingOverride: (data: {
    modelId: string
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }) => ipcRenderer.invoke('pricing:set-override', data),
  removePricingOverride: (modelId: string) => ipcRenderer.invoke('pricing:remove-override', modelId),
  getTimePricingRules: () => ipcRenderer.invoke('pricing:get-time-rules'),
  addTimePricingRule: (data: {
    modelId: string
    startTime: number
    endTime: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
    label: string
  }) => ipcRenderer.invoke('pricing:add-time-rule', data),
  updateTimePricingRule: (data: {
    id: number
    startTime: number
    endTime: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
    label: string
  }) => ipcRenderer.invoke('pricing:update-time-rule', data),
  deleteTimePricingRule: (id: number) => ipcRenderer.invoke('pricing:delete-time-rule', id),
  refreshPricing: () => ipcRenderer.invoke('pricing:refresh'),

  // 对话框
  openFileDialog: (filters?: { name: string; extensions: string[] }[]) =>
    ipcRenderer.invoke('dialog:open-file', { filters })
}

contextBridge.exposeInMainWorld('api', api)

export type ElectronAPI = typeof api
