import { ipcMain, dialog, BrowserWindow } from 'electron'
import { existsSync } from 'fs'
import { homedir } from 'os'
import { join } from 'path'
import type { ExternalDbService } from '../services/external-db'
import type { PricingEngine } from '../services/pricing-engine'
import type { AppDbService } from '../services/app-db'

let externalDb: ExternalDbService | null = null
let pricingEngine: PricingEngine | null = null
let appDb: AppDbService | null = null

export function setExternalDbForDialog(service: ExternalDbService): void {
  externalDb = service
}

export function setPricingEngineForDialog(engine: PricingEngine): void {
  pricingEngine = engine
}

export function setAppDbForDialog(db: AppDbService): void {
  appDb = db
}

// 注册文件对话框 IPC handler
export function registerDialogIPC(): void {
  // 选择数据库文件并自动加载
  ipcMain.handle('db:select-file', async () => {
    try {
      const win = BrowserWindow.getFocusedWindow()
      if (!win) {
        console.error('[db:select-file] 没有焦点窗口')
        return null
      }

      const result = await dialog.showOpenDialog(win, {
        title: '选择 CC-Switch 数据库文件',
        filters: [{ name: 'SQLite 数据库', extensions: ['db'] }],
        properties: ['openFile']
      })

      if (result.canceled || result.filePaths.length === 0) {
        console.log('[db:select-file] 用户取消选择')
        return null
      }

      const filePath = result.filePaths[0]
      console.log('[db:select-file] 选择的文件:', filePath)

      if (!externalDb) {
        console.error('[db:select-file] externalDb 未初始化')
        throw new Error('数据库服务未初始化')
      }

      // 打开外部数据库
      externalDb.open(filePath)
      console.log('[db:select-file] 数据库已打开')

      const count = externalDb.getRecordCount()
      console.log('[db:select-file] 记录数:', count)

      const dateRange = externalDb.getDateRange()
      console.log('[db:select-file] 日期范围:', dateRange)

      const providers = externalDb.getProviders()
      console.log('[db:select-file] 供应商数:', providers.length)

      const models = externalDb.getModels()
      console.log('[db:select-file] 模型数:', models.length)

      // 记录最新时间戳用于后续刷新检测
      const latest = externalDb.getLatestTimestamp()
      ;(global as any).__dbLatestTimestamp = latest

      // 加载外部数据库后刷新定价引擎
      if (pricingEngine) {
        try {
          pricingEngine.refresh()
          console.log('[db:select-file] 定价引擎已刷新')
        } catch (pe: any) {
          console.error('[db:select-file] 定价引擎刷新失败:', pe.message)
        }
      }

      // 记住选择的数据库路径
      if (appDb) {
        appDb.setSetting('last_db_path', filePath)
      }

      return {
        recordCount: count,
        dateRange: {
          min: dateRange.min,
          max: dateRange.max
        },
        providers,
        models
      }
    } catch (err: any) {
      console.error('[db:select-file] 异常:', err.message, err.stack)
      throw err
    }
  })

  // 自动加载默认数据库（优先使用记忆路径）
  ipcMain.handle('db:auto-load', async () => {
    const defaultPath = join(homedir(), '.cc-switch', 'cc-switch.db')
    const remembered = appDb?.getSetting('last_db_path')
    const dbPath = (remembered && existsSync(remembered)) ? remembered : defaultPath
    console.log('[db:auto-load] 路径:', dbPath)

    if (!existsSync(dbPath)) {
      console.log('[db:auto-load] 数据库不存在')
      return null
    }

    if (!externalDb) {
      console.error('[db:auto-load] externalDb 未初始化')
      return null
    }

    try {
      externalDb.open(dbPath)
      const count = externalDb.getRecordCount()
      const dateRange = externalDb.getDateRange()
      const providers = externalDb.getProviders()
      const models = externalDb.getModels()

      const latest = externalDb.getLatestTimestamp()
      ;(global as any).__dbLatestTimestamp = latest

      if (pricingEngine) {
        try { pricingEngine.refresh() } catch (pe: any) { /* ignore */ }
      }

      console.log('[db:auto-load] 自动加载成功, 记录数:', count)
      return {
        path: dbPath,
        recordCount: count,
        dateRange: { min: dateRange.min, max: dateRange.max },
        providers,
        models
      }
    } catch (err: any) {
      console.error('[db:auto-load] 加载失败:', err.message)
      return null
    }
  })

  // 通用文件打开对话框
  ipcMain.handle('dialog:open-file', async (_event, options?: { filters?: { name: string; extensions: string[] }[] }) => {
    const win = BrowserWindow.getFocusedWindow()
    const result = await dialog.showOpenDialog(win!, {
      title: '选择文件',
      filters: options?.filters || [{ name: '所有文件', extensions: ['*'] }],
      properties: ['openFile']
    })

    if (result.canceled || result.filePaths.length === 0) {
      return null
    }
    return result.filePaths[0]
  })
}
