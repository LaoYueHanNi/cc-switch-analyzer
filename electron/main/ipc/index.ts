import { ExternalDbService } from '../services/external-db'
import { AppDbService } from '../services/app-db'
import { PricingEngine } from '../services/pricing-engine'
import { registerDatabaseIPC, setExternalDb, setPricingEngineForDb } from './database.ipc'
import { registerDialogIPC, setExternalDbForDialog, setPricingEngineForDialog, setAppDbForDialog } from './dialog.ipc'
import { registerPricingIPC } from './pricing.ipc'
import { registerSessionTitleIPC } from './session-title.ipc'

// 全局服务实例
let externalDb: ExternalDbService
let appDb: AppDbService
let pricingEngine: PricingEngine

// 初始化所有 IPC 处理器和服务
export function initIPC(): { externalDb: ExternalDbService; appDb: AppDbService; pricingEngine: PricingEngine } {
  externalDb = new ExternalDbService()
  appDb = new AppDbService()
  pricingEngine = new PricingEngine(appDb)

  // 异步拉取云端定价并刷新
  pricingEngine.fetchAndCacheCloudPricing().then(() => {
    pricingEngine.refresh()
  }).catch(() => {})

  // 注：pricingEngine.refresh() 在数据库选择后由 dialog.ipc 触发

  // 注入服务到各 IPC 模块
  setExternalDb(externalDb)
  setExternalDbForDialog(externalDb)
  setPricingEngineForDb(pricingEngine)
  setPricingEngineForDialog(pricingEngine)
  setAppDbForDialog(appDb)

  // 注册所有 IPC handler
  registerDialogIPC()
  registerDatabaseIPC()
  registerPricingIPC(appDb, pricingEngine, () => externalDb)
  registerSessionTitleIPC(appDb)

  return { externalDb, appDb, pricingEngine }
}

// 获取服务实例
export function getExternalDb(): ExternalDbService {
  if (!externalDb) throw new Error('IPC 未初始化')
  return externalDb
}

export function getAppDb(): AppDbService {
  if (!appDb) throw new Error('IPC 未初始化')
  return appDb
}

export function getPricingEngine(): PricingEngine {
  if (!pricingEngine) throw new Error('IPC 未初始化')
  return pricingEngine
}
