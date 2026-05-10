// 应用常量定义

// 应用自有数据库路径
export const APP_DB_DIR = (() => {
  const os = require('os')
  return require('path').join(os.homedir(), '.cc-switch-analyzer')
})()

export const APP_DB_PATH = require('path').join(APP_DB_DIR, 'pricing.db')

// 云端定价文件 URL（Gitee raw 文件地址）
export const CLOUD_PRICING_URL = 'https://gitee.com/oyw125/model-price-table/raw/master/model_pricing.json'

// 查询版本号（用于防竞态）
export const QUERY_VERSION = 1

// 会话分析 Top N
export const SESSION_TOP_N = 50

// 实时监控窗口（秒）
export const REALTIME_WINDOW_SEC = 3600
