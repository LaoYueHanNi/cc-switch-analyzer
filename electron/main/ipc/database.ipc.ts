import { ipcMain, BrowserWindow } from 'electron'
import type { ExternalDbService, FilterParams } from '../services/external-db'
import type { PricingEngine } from '../services/pricing-engine'
import { precomputeCosts, computeSessionCosts, computeSessionModelCosts } from '../services/precompute'

// 全局服务引用（由 index.ts 注入）
let externalDb: ExternalDbService | null = null
let pricingEngine: PricingEngine | null = null

export function setExternalDb(service: ExternalDbService): void {
  externalDb = service
}

export function setPricingEngineForDb(engine: PricingEngine): void {
  pricingEngine = engine
}

// 注册所有数据库和查询 IPC handler
export function registerDatabaseIPC(): void {
  // ===== 数据库操作 =====

  ipcMain.handle('db:load', (_event, filePath: string) => {
    if (!externalDb) throw new Error('ExternalDbService 未初始化')
    externalDb.open(filePath)
    const count = externalDb.getRecordCount()
    const dateRange = externalDb.getDateRange()

    return {
      path: filePath,
      recordCount: count,
      dateRange: {
        min: dateRange.min,
        max: dateRange.max
      },
      providers: externalDb.getProviders(),
      models: externalDb.getModels()
    }
  })

  ipcMain.handle('db:refresh', () => {
    if (!externalDb || !externalDb.isOpen) return { hasNew: false }

    const prevMax = (global as any).__dbLatestTimestamp
    const currentMax = externalDb.getLatestTimestamp()

    if (currentMax !== null && prevMax !== currentMax) {
      (global as any).__dbLatestTimestamp = currentMax
      const count = externalDb.getRecordCount()
      return { hasNew: true, recordCount: count }
    }
    return { hasNew: false }
  })

  ipcMain.handle('db:get-filter-options', () => {
    if (!externalDb || !externalDb.isOpen) return { providers: [], models: [], dateRange: { min: 0, max: 0 } }
    const dateRange = externalDb.getDateRange()
    return {
      providers: externalDb.getProviders(),
      models: externalDb.getModels(),
      dateRange: {
        min: dateRange.min,
        max: dateRange.max
      }
    }
  })

  // ===== 数据查询 =====

  ipcMain.handle('query:summary', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getSummary(params)
  })

  ipcMain.handle('query:by-model', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getModelBreakdown(params)
  })

  ipcMain.handle('query:by-provider', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getProviderBreakdown(params)
  })

  ipcMain.handle('query:provider-model-tokens', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getProviderModelTokens(params)
  })

  ipcMain.handle('query:daily-trend', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getDailyTrend(params)
  })

  ipcMain.handle('query:cache-durations', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    const map = externalDb.getCacheNonDecayDuration(params)
    // 将 Map 序列化为对象
    return Object.fromEntries(map)
  })

  ipcMain.handle('query:cache-windows', (_event, modelId: string) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getRecentCacheWindows(modelId)
  })

  ipcMain.handle('query:sessions', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getSessionBreakdown(params)
  })

  ipcMain.handle('query:session-model-tokens', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getSessionModelTokens(params)
  })

  ipcMain.handle('query:session-request-tokens', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getSessionRequestTokens(params)
  })

  ipcMain.handle('query:session-timestamps', (_event, sessionIds: string[]) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    const map = externalDb.getSessionTimestamps(sessionIds)
    return Object.fromEntries(map)
  })

  ipcMain.handle('query:realtime', () => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    return externalDb.getMinuteLevelTokenTrend()
  })

  ipcMain.handle('query:realtime-logs', () => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    if (!pricingEngine) throw new Error('定价引擎未初始化')
    const raw = externalDb.getRecentRequestLogsRaw()
    return raw.map(r => {
      const p = pricingEngine!.getPricingAt(r.model, r.createdAt)
      const inputCost = p ? r.inputTokens * p.inputCostPerMillion / 1_000_000 : 0
      const outputCost = p ? r.outputTokens * p.outputCostPerMillion / 1_000_000 : 0
      const cacheReadCost = p ? r.cacheReadTokens * p.cacheReadCostPerMillion / 1_000_000 : 0
      const cacheCreationCost = p ? r.cacheCreationTokens * p.cacheCreationCostPerMillion / 1_000_000 : 0
      return {
        ...r,
        inputCost,
        outputCost,
        cacheReadCost,
        cacheCreationCost,
        totalCost: inputCost + outputCost + cacheReadCost + cacheCreationCost
      }
    })
  })

  // ===== 预计算查询（组合查询 + 费用计算） =====

  ipcMain.handle('query:precompute', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    if (!pricingEngine) throw new Error('定价引擎未初始化')

    // 查询数据
    const summary = externalDb.getSummary(params)
    const modelBreakdown = externalDb.getModelBreakdown(params)
    const providerBreakdown = externalDb.getProviderBreakdown(params)
    const dailyTrend = externalDb.getDailyTrend(params)
    const providerModelTokens = externalDb.getProviderModelTokens(params)
    const cacheDurations = externalDb.getCacheNonDecayDuration(params)

    // 预计算费用
    const precomputed = precomputeCosts(dailyTrend, providerModelTokens, pricingEngine)

    return {
      summary,
      modelBreakdown,
      providerBreakdown,
      precomputed: {
        ...precomputed,
        cacheDurations: Object.fromEntries(cacheDurations)
      }
    }
  })

  // 带费用计算的会话查询
  ipcMain.handle('query:sessions-with-cost', (_event, params: FilterParams) => {
    if (!externalDb || !externalDb.isOpen) throw new Error('数据库未打开')
    if (!pricingEngine) throw new Error('定价引擎未初始化')

    console.log('[sessions-with-cost] 查询会话, params:', JSON.stringify(params))
    const sessions = externalDb.getSessionBreakdown(params)
    console.log('[sessions-with-cost] 会话数:', sessions.length)
    const sessionRequestTokens = externalDb.getSessionRequestTokens(params)
    console.log('[sessions-with-cost] 请求级Token数据:', sessionRequestTokens.length)
    const sessionModelTokens = externalDb.getSessionModelTokens(params)
    console.log('[sessions-with-cost] 模型级Token数据:', sessionModelTokens.length)

    // 时间感知费用计算
    const sessionCosts = computeSessionCosts(sessionRequestTokens, pricingEngine)
    const sessionModelCosts = computeSessionModelCosts(sessionRequestTokens, sessionModelTokens, pricingEngine)

    // 提取 session IDs（Top 50 → Top 20）
    const topSessionIds = sessions.slice(0, 20).map(s => s.sessionId)

    // 获取时间戳
    const timestampsMap = externalDb.getSessionTimestamps(topSessionIds)

    // 组装会话数据
    const enrichedSessions = sessions.slice(0, 20).map(s => {
      const cost = sessionCosts[s.sessionId] || 0
      const modelCosts = sessionModelCosts[s.sessionId] || {}
      const timestamps = timestampsMap.get(s.sessionId) || []
      const durationSec = s.lastAt - s.firstAt

      // 缓存命中率
      const totalCacheRead = s.cacheRead
      const totalInput = s.inputTokens
      const cacheHitRate = (totalInput + totalCacheRead) > 0
        ? totalCacheRead / (totalInput + totalCacheRead)
        : 0

      return {
        sessionId: s.sessionId,
        requestCount: s.requests,
        totalTokens: s.inputTokens + s.outputTokens + s.cacheRead + s.cacheCreation,
        maxContextWidth: s.maxContextWidth || 0,
        startTime: s.firstAt,
        endTime: s.lastAt,
        cacheHitRate,
        totalCost: cost,
        durationSec,
        timestamps,
        modelBreakdown: Object.entries(modelCosts).map(([model, mc]) => ({
          sessionId: s.sessionId,
          model,
          cost: mc.cost,
          inputTokens: (mc as any).tokens?.input || 0,
          outputTokens: (mc as any).tokens?.output || 0,
          cacheReadTokens: (mc as any).tokens?.cacheRead || 0,
          cacheCreationTokens: (mc as any).tokens?.cacheCreation || 0,
          inputCost: mc.breakdown[0],
          outputCost: mc.breakdown[1],
          cacheReadCost: mc.breakdown[2],
          cacheCreationCost: mc.breakdown[3]
        }))
      }
    })

    return enrichedSessions
  })
}
