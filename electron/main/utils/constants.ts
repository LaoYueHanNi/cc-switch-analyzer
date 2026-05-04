// 应用常量定义

// 应用自有数据库路径
export const APP_DB_DIR = (() => {
  const os = require('os')
  return require('path').join(os.homedir(), '.cc-switch-analyzer')
})()

export const APP_DB_PATH = require('path').join(APP_DB_DIR, 'pricing.db')

// 默认汇率
export const DEFAULT_EXCHANGE_RATE = 7.0

// 查询版本号（用于防竞态）
export const QUERY_VERSION = 1

// 缓存窗口历史范围（秒）
export const CACHE_WINDOW_DAYS = 30

// 会话分析 Top N
export const SESSION_TOP_N = 50

// 实时监控窗口（秒）
export const REALTIME_WINDOW_SEC = 3600
