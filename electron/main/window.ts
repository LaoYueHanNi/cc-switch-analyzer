import { BrowserWindow, app } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'fs'

// 窗口状态持久化文件路径
const statePath = join(app.getPath('userData'), 'window-state.json')

interface WindowState {
  x?: number
  y?: number
  w: number
  h: number
  maximized: boolean
}

// 读取保存的窗口状态
function loadState(): WindowState {
  try {
    if (existsSync(statePath)) {
      const data = JSON.parse(readFileSync(statePath, 'utf-8'))
      return {
        x: data.x,
        y: data.y,
        w: data.w || 1280,
        h: data.h || 860,
        maximized: data.maximized || false
      }
    }
  } catch {
    // 文件损坏则使用默认值
  }
  return { w: 1280, h: 860, maximized: false }
}

// 保存窗口状态
function saveState(win: BrowserWindow): void {
  try {
    const bounds = win.getBounds()
    const maximized = win.isMaximized()
    const dir = join(app.getPath('userData'))
    if (!existsSync(dir)) mkdirSync(dir, { recursive: true })
    writeFileSync(
      statePath,
      JSON.stringify({
        x: bounds.x,
        y: bounds.y,
        w: bounds.width,
        h: bounds.height,
        maximized
      })
    )
  } catch {
    // 忽略保存失败
  }
}

// 创建主窗口，恢复上次窗口状态
export function createMainWindow(): BrowserWindow {
  const state = loadState()

  const win = new BrowserWindow({
    x: state.x,
    y: state.y,
    width: state.w,
    height: state.h,
    show: false,
    title: 'CC-Switch 使用分析器',
    webPreferences: {
      preload: join(__dirname, '../preload/index.mjs'),
      sandbox: false
    }
  })

  // 恢复最大化状态
  if (state.maximized) {
    win.maximize()
  }

  // 监听窗口变化，保存状态
  win.on('close', () => saveState(win))
  win.on('resize', () => saveState(win))
  win.on('move', () => saveState(win))

  win.on('ready-to-show', () => {
    win.show()
  })

  return win
}
