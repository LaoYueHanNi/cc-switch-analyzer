// UI 常量（参考 Java 版 Styles.java + AppConstants.java）

// 摘要统计条指标配置（颜色使用 CSS 变量以支持暗色模式）
export const SUMMARY_ITEMS = [
  { key: 'totalRequests', label: '总请求数', color: 'var(--color-blue)' },
  { key: 'totalCost', label: '总费用（¥）', color: 'var(--color-cost)' },
  { key: 'totalInput', label: '输入', color: 'var(--color-purple)' },
  { key: 'totalOutput', label: '输出', color: 'var(--color-orange)' },
  { key: 'totalCacheRead', label: '缓存命中', color: 'var(--color-blue)' },
  { key: 'totalCacheCreation', label: '缓存写入', color: 'var(--color-dark-orange)' },
  { key: 'totalTokens', label: '总Token', color: 'var(--color-green)' }
] as const

// 自动刷新间隔选项
export const REFRESH_INTERVAL_OPTIONS = [
  { label: '手动', value: 'manual' },
  { label: '30秒', value: '30s' },
  { label: '1分钟', value: '1min' },
  { label: '5分钟', value: '5min' },
  { label: '30分钟', value: '30min' }
] as const
