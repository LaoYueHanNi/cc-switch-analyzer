// UI 常量（参考 Java 版 Styles.java + AppConstants.java）

// 功能色
export const COLORS = {
  COST_RED: '#e74c3c',
  PRIMARY_BLUE: '#4a90d9',
  GREEN: '#27ae60',
  PURPLE: '#8e44ad',
  ORANGE: '#f39c12',
  TEAL: '#16a085',
  BLUE: '#2980b9',
  DARK_ORANGE: '#d35400'
} as const

// 摘要统计条 8 项指标配置
export const SUMMARY_ITEMS = [
  { key: 'totalRequests', label: '总请求数', color: COLORS.PRIMARY_BLUE },
  { key: 'successCount', label: '成功请求数', color: COLORS.GREEN },
  { key: 'totalCost', label: '总费用（¥）', color: COLORS.COST_RED },
  { key: 'totalInput', label: '输入 Token', color: COLORS.PURPLE },
  { key: 'totalOutput', label: '输出 Token', color: COLORS.ORANGE },
  { key: 'avgLatency', label: '平均延迟(ms)', color: COLORS.TEAL },
  { key: 'totalCacheRead', label: '缓存命中 Token', color: COLORS.BLUE },
  { key: 'totalCacheCreation', label: '缓存写入 Token', color: COLORS.DARK_ORANGE }
] as const

// 默认汇率
export const DEFAULT_EXCHANGE_RATE = 7.0

// 自动刷新间隔选项
export const REFRESH_INTERVAL_OPTIONS = [
  { label: '手动', value: 'manual' },
  { label: '30秒', value: '30s' },
  { label: '1分钟', value: '1min' },
  { label: '5分钟', value: '5min' },
  { label: '30分钟', value: '30min' }
] as const
