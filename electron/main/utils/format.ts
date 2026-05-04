// 数值格式化工具函数（主进程）

// 大数 K/M 简写
export function formatNum(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(n)
}

// 定价单价格式化：2-4 位小数，去尾零，保底 2 位
export function formatRate(v: number): string {
  let s = v.toFixed(4)
  const dot = s.indexOf('.')
  if (dot < 0) return s
  s = s.replace(/0+$/, '')
  if (s.endsWith('.')) s += '00'
  const minEnd = dot + 3
  while (s.length < minEnd) s += '0'
  return s
}

// 费用格式化
export function formatCost(cny: number): string {
  return '¥' + formatRate(cny)
}

// 时长格式化
export function formatDuration(seconds: number): string {
  if (seconds < 60) return seconds + 's'
  if (seconds < 3600) return Math.floor(seconds / 60) + 'm'
  return Math.floor(seconds / 3600) + 'h ' + Math.floor((seconds % 3600) / 60) + 'm'
}

// 日期转为 Unix 秒（当天 00:00:00 UTC）
export function toEpochSeconds(date: Date): number {
  // 使用 UTC 日期，确保与 Java 行为一致
  const utcDate = new Date(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()))
  return Math.floor(utcDate.getTime() / 1000)
}

// 当天 23:59:59 UTC（toDate 的 exclusive 处理：+1 天的 00:00:00）
export function toExclusiveEndEpoch(date: Date): number {
  const nextDay = new Date(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate() + 1))
  return Math.floor(nextDay.getTime() / 1000)
}

// Unix 秒转为 YYYY-MM-DD
export function epochToDateStr(epoch: number): string {
  const d = new Date(epoch * 1000)
  return d.toISOString().slice(0, 10)
}

// 从 YYYY-MM-DD 字符串转为 Unix 秒
export function dateStrToEpoch(dateStr: string): number {
  const d = new Date(dateStr + 'T00:00:00Z')
  return Math.floor(d.getTime() / 1000)
}
