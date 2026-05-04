import { ipcMain } from 'electron'
import type { AppDbService } from '../services/app-db'
import type { PricingEngine } from '../services/pricing-engine'

// 注册定价相关 IPC handler
export function registerPricingIPC(appDb: AppDbService, pricingEngine: PricingEngine, getExternalDb: () => any): void {
  // 获取汇率
  ipcMain.handle('pricing:get-exchange-rate', () => {
    return appDb.getExchangeRate()
  })

  // 设置汇率
  ipcMain.handle('pricing:set-exchange-rate', (_event, rate: number) => {
    appDb.setExchangeRate(rate)
  })

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
  ipcMain.handle('pricing:delete-time-rule', (_event, id: number) => {
    appDb.deleteTimeOverride(id)
  })

  // 刷新定价（级联刷新）
  ipcMain.handle('pricing:refresh', () => {
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
      isUsed: usedModels.has(p.modelId)
    }))
  })
}
