import { describe, it, expect } from 'vitest'
import {
  formatNum,
  formatRate,
  formatCost,
  formatDuration,
  epochToDateStr,
  epochToTimeStr,
  epochToDateTimeStr,
  formatPercent
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
// epochToDateStr — Unix 秒 → YYYY-MM-DD
// ---------------------------------------------------------------------------
describe('epochToDateStr（渲染进程）', () => {
  it('已知 epoch 值', () => {
    // 1704067200 = 2024-01-01T00:00:00Z
    expect(epochToDateStr(1704067200)).toBe('2024-01-01')
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
