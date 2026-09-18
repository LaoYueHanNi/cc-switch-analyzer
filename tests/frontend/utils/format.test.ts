import { describe, it, expect } from 'vitest'
import {
  formatNum,
  formatRate,
  formatCost,
  formatDuration,
  epochToDateStr,
  epochToTimeStr,
  epochToDateTimeStr,
  formatPercent,
  shortSessionId,
  formatTokenSpeed,
  formatDualTokenSpeed,
  formatLatency,
  formatTtft
} from '@/utils/format'

// ---------------------------------------------------------------------------
// formatNum — 大数 K/M 简写（使用 Math.floor 截断小数）
// ---------------------------------------------------------------------------
describe('formatNum（渲染进程）', () => {
  it('0 → "0"', () => {
    expect(formatNum(0)).toBe('0')
  })

  it('500 → "500"', () => {
    expect(formatNum(500)).toBe('500')
  })

  it('999 → "999"', () => {
    expect(formatNum(999)).toBe('999')
  })

  it('1000 → "1.0K"', () => {
    expect(formatNum(1000)).toBe('1.0K')
  })

  it('1500 → "1.5K"', () => {
    expect(formatNum(1500)).toBe('1.5K')
  })

  it('1_000_000 → "1.0M"', () => {
    expect(formatNum(1_000_000)).toBe('1.0M')
  })

  it('2_500_000 → "2.5M"', () => {
    expect(formatNum(2_500_000)).toBe('2.5M')
  })

  it('1.5 → "1"（Math.floor 截断小数）', () => {
    expect(formatNum(1.5)).toBe('1')
  })

  it('负数 -100 → "-100"', () => {
    expect(formatNum(-100)).toBe('-100')
  })
})

// ---------------------------------------------------------------------------
// formatRate — 与主进程逻辑相同
// ---------------------------------------------------------------------------
describe('formatRate（渲染进程）', () => {
  it('3.0 → "3.00"', () => {
    expect(formatRate(3.0)).toBe('3.00')
  })

  it('3.14 → "3.14"', () => {
    expect(formatRate(3.14)).toBe('3.14')
  })

  it('3.1415 → "3.1415"', () => {
    expect(formatRate(3.1415)).toBe('3.1415')
  })

  it('0 → "0.00"', () => {
    expect(formatRate(0)).toBe('0.00')
  })
})

// ---------------------------------------------------------------------------
// formatCost — 使用 toFixed(2)，与主进程 formatRate 不同
// ---------------------------------------------------------------------------
describe('formatCost（渲染进程）', () => {
  it('3.14 → "¥3.14"', () => {
    expect(formatCost(3.14)).toBe('¥3.14')
  })

  it('0 → "¥0.00"', () => {
    expect(formatCost(0)).toBe('¥0.00')
  })

  it('3.14159 → "¥3.14"（toFixed(2) 截断，不做 formatRate 那样保留 4 位）', () => {
    expect(formatCost(3.14159)).toBe('¥3.14')
  })
})

// ---------------------------------------------------------------------------
// formatDuration — 与主进程逻辑相同
// ---------------------------------------------------------------------------
describe('formatDuration（渲染进程）', () => {
  it('0 → "0s"', () => {
    expect(formatDuration(0)).toBe('0s')
  })

  it('30 → "30s"', () => {
    expect(formatDuration(30)).toBe('30s')
  })

  it('59 → "59s"', () => {
    expect(formatDuration(59)).toBe('59s')
  })

  it('60 → "1m"', () => {
    expect(formatDuration(60)).toBe('1m')
  })

  it('90 → "1m"', () => {
    expect(formatDuration(90)).toBe('1m')
  })

  it('3599 → "59m"', () => {
    expect(formatDuration(3599)).toBe('59m')
  })

  it('3600 → "1h 0m"', () => {
    expect(formatDuration(3600)).toBe('1h 0m')
  })

  it('3661 → "1h 1m"', () => {
    expect(formatDuration(3661)).toBe('1h 1m')
  })

  it('86399 → "23h 59m"', () => {
    expect(formatDuration(86399)).toBe('23h 59m')
  })
})

// ---------------------------------------------------------------------------
// formatPercent — 百分比格式化
// ---------------------------------------------------------------------------
describe('formatPercent（渲染进程）', () => {
  it('0.5 → "50.0%"', () => {
    expect(formatPercent(0.5)).toBe('50.0%')
  })

  it('0 → "0.0%"', () => {
    expect(formatPercent(0)).toBe('0.0%')
  })

  it('1 → "100.0%"', () => {
    expect(formatPercent(1)).toBe('100.0%')
  })

  it('0.1234 → "12.3%"（toFixed(1) 四舍五入）', () => {
    expect(formatPercent(0.1234)).toBe('12.3%')
  })
})

// ---------------------------------------------------------------------------
// epochToDateStr — Unix 秒 → YYYY-MM-DD（按本地时区）
// ---------------------------------------------------------------------------
describe('epochToDateStr（渲染进程）', () => {
  it('已知 epoch 值（正午 UTC，跨时区稳定）', () => {
    // 1704110400 = 2024-01-01T12:00:00Z
    // 选正午 UTC 是为了在 UTC ±12h 范围内各时区日期都是 2024-01-01，
    // 避免运行时区差异导致断言失败
    expect(epochToDateStr(1704110400)).toBe('2024-01-01')
  })
})

// ---------------------------------------------------------------------------
// epochToTimeStr — Unix 秒 → HH:mm（zh-CN locale）
// ---------------------------------------------------------------------------
describe('epochToTimeStr（渲染进程）', () => {
  it('返回格式匹配 HH:mm', () => {
    // 2024-01-01T12:00:00Z = 1704110400
    const result = epochToTimeStr(1704110400)
    expect(result).toMatch(/\d{2}:\d{2}/)
  })

  it('午夜零点（UTC）', () => {
    // 2024-01-01T00:00:00Z = 1704067200
    // toLocaleTimeString 受运行环境时区影响，不硬编码精确值
    // 只验证 HH:mm 格式
    const result = epochToTimeStr(1704067200)
    expect(result).toMatch(/\d{2}:\d{2}/)
  })
})

// ---------------------------------------------------------------------------
// epochToDateTimeStr — Unix 秒 → MM/DD HH:mm
// ---------------------------------------------------------------------------
describe('epochToDateTimeStr（渲染进程）', () => {
  it('返回格式匹配 MM/DD HH:mm', () => {
    // 2024-01-15T12:00:00Z = 1705276800
    const result = epochToDateTimeStr(1705276800)
    expect(result).toMatch(/^\d{2}\/\d{2}\s\d{2}:\d{2}$/)
  })

  it('具体日期验证', () => {
    // 2024-01-15T12:00:00Z = 1705276800 → "01/15 12:00"（UTC）
    const result = epochToDateTimeStr(1705276800)
    expect(result).toContain('01/15')
    expect(result).toMatch(/\d{2}:\d{2}/)
  })
})

// ---------------------------------------------------------------------------
// shortSessionId — 会话/任务 ID 短显示
// ---------------------------------------------------------------------------
describe('shortSessionId（渲染进程）', () => {
  it('ses_ 前缀 → 取前 8 位', () => {
    expect(shortSessionId('ses_abcdefghijklmnop')).toBe('ses_abcd')
  })

  it('UUID → 取第一段（连字符前）', () => {
    expect(shortSessionId('550e8400-e29b-41d4-a716-446655440000')).toBe('550e8400')
  })

  it('无连字符且不带 ses_ 前缀 → split 无分隔符，原样返回', () => {
    expect(shortSessionId('plainidwithoutdash')).toBe('plainidwithoutdash')
  })

  it('短于 8 位的普通 ID → 原样返回', () => {
    expect(shortSessionId('abc')).toBe('abc')
  })
})

// ---------------------------------------------------------------------------
// formatTokenSpeed — Token 输出速度格式化
// ---------------------------------------------------------------------------
describe('formatTokenSpeed（渲染进程）', () => {
  it('0 输出或 0 延迟 → "-"', () => {
    expect(formatTokenSpeed(0, 1000)).toBe('-')
    expect(formatTokenSpeed(100, 0)).toBe('-')
    expect(formatTokenSpeed(0, 0)).toBe('-')
    expect(formatTokenSpeed(-10, 1000)).toBe('-')
    expect(formatTokenSpeed(100, -500)).toBe('-')
  })

  it('双轨速度计算：无首字或首字为0时纯吐字显示为 -，总速度正常计算', () => {
    // 100 tokens, 1000ms → A: '-', B: '100.0' → '-/100.0 tok/s'
    expect(formatTokenSpeed(100, 1000)).toBe('-/100.0 tok/s')
    expect(formatTokenSpeed(100, 1000, 0)).toBe('-/100.0 tok/s')
    expect(formatTokenSpeed(100, 1000, null)).toBe('-/100.0 tok/s')
    // 50 tokens, 2000ms = -/25.0 tok/s
    expect(formatTokenSpeed(50, 2000)).toBe('-/25.0 tok/s')
  })

  it('排除首字时间计算纯吐字速率：只要耗时大于首字时间（streamingMs > 0）即如实计算', () => {
    // 100 tokens, latency 2000ms, ttft 1000ms → 纯流式 1000ms (100.0 tok/s)，总速度 2000ms (50.0 tok/s)
    expect(formatTokenSpeed(100, 2000, 1000)).toBe('100.0/50.0 tok/s')
    // 96 tokens, latency 6300ms, ttft 5500ms（差 800ms < 1s）：96*1000/800 = 120.0 tok/s
    expect(formatTokenSpeed(96, 6300, 5500)).toBe('120.0/15.2 tok/s')
    // 50 tokens, latency 2500ms, ttft 500ms → 纯流式 2000ms (25.0 tok/s)，总速度 2500ms (20.0 tok/s)
    expect(formatTokenSpeed(50, 2500, 500)).toBe('25.0/20.0 tok/s')
  })

  it('首字时间异常或等于耗时（streamingMs <= 0）时，纯吐字标记为 -，总速度正常展示', () => {
    // 若 ttft >= latencyMs 异常，纯吐字标记为 -，总速度正常
    expect(formatTokenSpeed(100, 1000, 1000)).toBe('-/100.0 tok/s')
    expect(formatTokenSpeed(100, 1000, 1500)).toBe('-/100.0 tok/s')
  })
})

// ---------------------------------------------------------------------------
// formatDualTokenSpeed — 结构化双轨速度与 Tooltip
// ---------------------------------------------------------------------------
describe('formatDualTokenSpeed（双轨结构与 Tooltip）', () => {
  it('无数据或非正数返回 "-"', () => {
    expect(formatDualTokenSpeed(0, 1000)).toEqual({ speedA: '-', speedB: '-', text: '-', tooltip: '' })
    expect(formatDualTokenSpeed(100, 0)).toEqual({ speedA: '-', speedB: '-', text: '-', tooltip: '' })
  })

  it('有效流式源：speedA 与 speedB 均有效，tooltip 包含两项速度说明', () => {
    // 100 tokens, latency 2000ms, ttft 1000ms
    const res = formatDualTokenSpeed(100, 2000, 1000)
    expect(res.speedA).toBe('100.0')
    expect(res.speedB).toBe('50.0')
    expect(res.text).toBe('100.0/50.0 tok/s')
    expect(res.tooltip).toBe('输出速度 (纯吐字): 100.0 tok/s\n总速度 (端到端): 50.0 tok/s')
  })

  it('源头无首字时：speedA 为 "-"，tooltip 提示无首字', () => {
    const res = formatDualTokenSpeed(100, 2000, null)
    expect(res.speedA).toBe('-')
    expect(res.speedB).toBe('50.0')
    expect(res.text).toBe('-/50.0 tok/s')
    expect(res.tooltip).toContain('输出速度 (纯吐字): 无首字')
    expect(res.tooltip).toContain('总速度 (端到端): 50.0 tok/s')
  })

  it('首字时间异常或等于耗时（ttft >= latency）：speedA 为 "-"', () => {
    const res = formatDualTokenSpeed(100, 1000, 1000)
    expect(res.speedA).toBe('-')
    expect(res.speedB).toBe('100.0')
    expect(res.text).toBe('-/100.0 tok/s')
  })
})

// ---------------------------------------------------------------------------
// formatLatency — 耗时 / 延迟格式化
// ---------------------------------------------------------------------------
describe('formatLatency（渲染进程）', () => {
  it('小于 1000ms 显示为 Xms', () => {
    expect(formatLatency(50)).toBe('50ms')
    expect(formatLatency(999)).toBe('999ms')
  })

  it('无值、0 或负数返回 "-"（0 表示源头未采集耗时，不是瞬时完成）', () => {
    expect(formatLatency(0)).toBe('-')
    expect(formatLatency(-1)).toBe('-')
    expect(formatLatency(undefined)).toBe('-')
    expect(formatLatency(null as any)).toBe('-')
  })

  it('>= 1000ms 显示为 X.Xs', () => {
    expect(formatLatency(1000)).toBe('1.0s')
    expect(formatLatency(1500)).toBe('1.5s')
    expect(formatLatency(4230)).toBe('4.2s')
  })
})

// ---------------------------------------------------------------------------
// formatTtft — 首字耗时格式化
// ---------------------------------------------------------------------------
describe('formatTtft（渲染进程）', () => {
  it('无值、0 或负数返回 "-"', () => {
    expect(formatTtft(undefined)).toBe('-')
    expect(formatTtft(null as any)).toBe('-')
    expect(formatTtft(0)).toBe('-')
    expect(formatTtft(-100)).toBe('-')
  })

  it('有效耗时按 latency 规则格式化', () => {
    expect(formatTtft(450)).toBe('450ms')
    expect(formatTtft(1200)).toBe('1.2s')
  })
})

