// 数值格式化工具函数（渲染进程）

// 大数 K/M 简写
export function formatNum(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(Math.floor(n))
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

// 费用格式化：¥X.XX，固定 2 位小数
export function formatCost(cny: number): string {
  return '¥' + cny.toFixed(2)
}

// 时长格式化
export function formatDuration(seconds: number): string {
  if (seconds < 60) return seconds + 's'
  if (seconds < 3600) return Math.floor(seconds / 60) + 'm'
  return Math.floor(seconds / 3600) + 'h ' + Math.floor((seconds % 3600) / 60) + 'm'
}

// Unix 秒转日期字符串
export function epochToDateStr(epoch: number): string {
  const d = new Date(epoch * 1000)
  return d.toISOString().slice(0, 10)
}

// Unix 秒转时间字符串 HH:mm
export function epochToTimeStr(epoch: number): string {
  const d = new Date(epoch * 1000)
  return d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
}

// Unix 秒转日期+时间
export function epochToDateTimeStr(epoch: number): string {
  const d = new Date(epoch * 1000)
  const month = (d.getMonth() + 1).toString().padStart(2, '0')
  const day = d.getDate().toString().padStart(2, '0')
  const time = d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
  return `${month}/${day} ${time}`
}

// 百分比格式化
export function formatPercent(rate: number): string {
  return (rate * 100).toFixed(1) + '%'
}

// 会话/任务 ID 短显示：ses_ 前缀取前 8 位，UUID 取第一段
export function shortSessionId(id: string): string {
  if (id.startsWith('ses_')) return id.slice(0, 8)
  const parts = id.split('-')
  return parts[0] || id.slice(0, 8)
}
