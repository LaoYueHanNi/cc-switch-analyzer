import { ipcMain } from 'electron'
import type { AppDbService } from '../services/app-db'
import { resolveSessionTitles } from '../services/session-title'

export function registerSessionTitleIPC(appDb: AppDbService): void {
  ipcMain.handle('session-title:get-titles', (_event, sessionIds: string[]) => {
    return Object.fromEntries(resolveSessionTitles(appDb, sessionIds))
  })
}
