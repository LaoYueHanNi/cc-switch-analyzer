import { ipcMain } from 'electron'
import type { AppDbService } from '../services/app-db'
import type { PricingEngine } from '../services/pricing-engine'

// 注册定价相关 IPC handler
export function registerPricingIPC(appDb: AppDbService, pricingEngine: PricingEngine, getExternalDb: () => any): void {
  // 获取所有定价覆盖
  ipcMain.handle('pricing:get-overrides', () => {
    return appDb.getAllOverrides()
  })

  // 设置定价覆盖
  ipcMain.handle('pricing:set-override', (_event, data: {
    modelId: string
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }) => {
    appDb.saveOverride(data.modelId, data.input, data.output, data.cacheRead, data.cacheCreation)
  })

  // 删除定价覆盖
  ipcMain.handle('pricing:remove-override', (_event, modelId: string) => {
    appDb.deleteOverride(modelId)
  })

  // 获取所有时间定价规则
  ipcMain.handle('pricing:get-time-rules', () => {
    return appDb.getAllTimeOverrides()
  })

  // 添加时间定价规则
  ipcMain.handle('pricing:add-time-rule', (_event, data: {
    modelId: string
    startTime: number
    endTime: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
    label: string
  }) => {
    return appDb.addTimeOverride(
      data.modelId,
      data.startTime,
      data.endTime,
      data.input,
      data.output,
      data.cacheRead,
      data.cacheCreation,
      data.label
    )
  })

  // 更新时间定价规则
  ipcMain.handle('pricing:update-time-rule', (_event, data: {
    id: number
    startTime: number
    endTime: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
    label: string
  }) => {
    appDb.updateTimeOverride(
      data.id,
      data.startTime,
      data.endTime,
      data.input,
      data.output,
      data.cacheRead,
      data.cacheCreation,
      data.label
    )
  })

  // 删除时间定价规则
  ipcMain.handle('pricing:delete-time-rule', (_event, data: {
    modelId: string
    startTime: number
    endTime: number
    id: number
  }) => {
    appDb.deleteTimeOverrideGroup(data.modelId, data.startTime, data.endTime)
  })

  // 保存覆盖上下文档位
  ipcMain.handle('pricing:save-override-tier', (_event, data: {
    modelId: string
    threshold: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }) => {
    appDb.saveOverrideTier(data.modelId, data.threshold, data.input, data.output, data.cacheRead, data.cacheCreation)
  })

  // 删除覆盖上下文档位
  ipcMain.handle('pricing:delete-override-tier', (_event, data: {
    modelId: string
    threshold: number
  }) => {
    appDb.deleteOverrideTier(data.modelId, data.threshold)
  })

  // 保存时间规则上下文档位
  ipcMain.handle('pricing:save-time-rule-tier', (_event, data: {
    modelId: string
    startTime: number
    endTime: number
    threshold: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }) => {
    return appDb.addTimeOverrideTier(
      data.modelId, data.startTime, data.endTime, data.threshold,
      data.input, data.output, data.cacheRead, data.cacheCreation
    )
  })

  // 更新时间规则上下文档位
  ipcMain.handle('pricing:update-time-rule-tier', (_event, data: {
    id: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }) => {
    appDb.updateTimeOverrideTier(data.id, data.input, data.output, data.cacheRead, data.cacheCreation)
  })

  // 删除时间规则上下文档位
  ipcMain.handle('pricing:delete-time-rule-tier', (_event, id: number) => {
    appDb.deleteTimeOverride(id)
  })

  // 刷新定价（级联刷新）
  ipcMain.handle('pricing:refresh', () => {
    pricingEngine.refresh()
  })

  // 拉取云端定价并刷新
  ipcMain.handle('pricing:fetch-cloud', async () => {
    await pricingEngine.fetchAndCacheCloudPricing()
    pricingEngine.refresh()
  })

  // 获取全部定价数据（合并后的，含 isUsed 标记）
  ipcMain.handle('pricing:get-all', () => {
    const all = pricingEngine.getAllPricing()

    // 从外部数据库获取实际使用过的模型列表
    let usedModels: Set<string> = new Set()
    const extDb = getExternalDb()
    if (extDb && extDb.isOpen) {
      const models = extDb.getModels()
      usedModels = new Set(models)
    }

    return all.map(p => ({
      modelId: p.modelId,
      displayName: p.displayName,
      inputCostPerMillion: p.inputCostPerMillion,
      outputCostPerMillion: p.outputCostPerMillion,
      cacheReadCostPerMillion: p.cacheReadCostPerMillion,
      cacheCreationCostPerMillion: p.cacheCreationCostPerMillion,
      isOverride: p.isOverride,
      hasTimePricing: pricingEngine.hasTimePricing(p.modelId),
      timeRules: pricingEngine.getTimeRules(p.modelId),
      isUsed: usedModels.has(p.modelId),
      contextTiers: pricingEngine.getOverrideTiers(p.modelId),
      cloudTimeRules: pricingEngine.getCloudTimeRules(p.modelId)
    }))
  })
}
