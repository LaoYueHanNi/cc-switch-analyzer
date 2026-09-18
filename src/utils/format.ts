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

// Unix 秒转日期字符串（YYYY-MM-DD，按本地时区，与 epochToDateTimeStr 等保持一致）
export function epochToDateStr(epoch: number): string {
  const d = new Date(epoch * 1000)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
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

export interface DualSpeedInfo {
  speedA: string // 纯吐字速率（如 "45.2" 或 "-"）
  speedB: string // 端到端总速度（如 "18.5" 或 "-"）
  text: string   // 如 "45.2/18.5 tok/s" 或 "-/18.5 tok/s" 或 "-"
  tooltip: string
}

/**
 * 格式化双轨输出速度：A/B tok/s
 * - A: 纯吐字速度（扣除首字），若无首字或 latencyMs <= ttft 则为 '-'
 * - B: 端到端总速度（基于整段耗时）
 */
export function formatDualTokenSpeed(
  outputTokens: number,
  latencyMs: number,
  timeToFirstToken?: number | null,
): DualSpeedInfo {
  if (!outputTokens || outputTokens <= 0 || !latencyMs || latencyMs <= 0) {
    return { speedA: '-', speedB: '-', text: '-', tooltip: '' }
  }

  const totalSpeed = (outputTokens * 1000) / latencyMs
  const speedB = totalSpeed.toFixed(1)

  let speedA = '-'
  const hasValidTtft =
    timeToFirstToken !== undefined &&
    timeToFirstToken !== null &&
    timeToFirstToken > 0 &&
    latencyMs > timeToFirstToken

  if (hasValidTtft) {
    const streamingMs = latencyMs - (timeToFirstToken as number)
    if (streamingMs > 0) {
      const streamSpeed = (outputTokens * 1000) / streamingMs
      speedA = streamSpeed.toFixed(1)
    }
  }

  const text = `${speedA}/${speedB} tok/s`
  const tooltip = `输出速度 (纯吐字): ${speedA === '-' ? '无首字' : `${speedA} tok/s`}\n总速度 (端到端): ${speedB} tok/s`

  return { speedA, speedB, text, tooltip }
}

// Token 输出速度格式化（纯文本双轨 A/B tok/s）：
export function formatTokenSpeed(
  outputTokens: number,
  latencyMs: number,
  timeToFirstToken?: number | null,
): string {
  return formatDualTokenSpeed(outputTokens, latencyMs, timeToFirstToken).text
}

// 延迟 / 耗时格式化：>= 1000ms 显示为 "X.Xs"，否则 "Xms"
// 无值或 <= 0 显示为 "-"：0 表示源头未采集耗时（如 CCS 的 session_log 来源
// 记录不带 latency_ms），真实请求耗时不可能为 0，显示 "0ms" 会被误读为瞬时完成
export function formatLatency(ms?: number | null): string {
  if (ms === undefined || ms === null || ms <= 0) return '-'
  if (ms >= 1000) return (ms / 1000).toFixed(1) + 's'
  return ms + 'ms'
}

// 首字时间（TTFT）格式化：有值且 > 0 时按耗时规则格式化，无值或 <= 0 显示为 "-"
export function formatTtft(ms?: number | null): string {
  return formatLatency(ms)
}

