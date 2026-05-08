import { describe, it, expect } from 'vitest'
import {
  formatNum,
  formatRate,
  formatCost,
  formatDuration,
  toEpochSeconds,
  toExclusiveEndEpoch,
  epochToDateStr,
  dateStrToEpoch
} from '../../../electron/main/utils/format'

// ---------------------------------------------------------------------------
// formatNum — 大数 K/M 简写（不做 Math.floor，保留小数）
// ---------------------------------------------------------------------------
describe('formatNum（主进程）', () => {
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

  it('1.5 → "1.5"（不做 Math.floor，保留小数）', () => {
    expect(formatNum(1.5)).toBe('1.5')
  })

  it('负数 -100 → "-100"', () => {
    expect(formatNum(-100)).toBe('-100')
  })

  it('负数 -1500 → "-1500"（负数不触发 K/M 缩写）', () => {
    expect(formatNum(-1500)).toBe('-1500')
  })

  it('负数 -2500000 → "-2500000"（负数不触发 K/M 缩写）', () => {
    expect(formatNum(-2_500_000)).toBe('-2500000')
  })
})

// ---------------------------------------------------------------------------
// formatRate — 定价单价格式化：2-4 位小数，去尾零，保底 2 位
// ---------------------------------------------------------------------------
describe('formatRate（主进程）', () => {
  it('3.0 → "3.00"（补齐 2 位小数）', () => {
    expect(formatRate(3.0)).toBe('3.00')
  })

  it('3.14 → "3.14"（恰好 2 位小数）', () => {
    expect(formatRate(3.14)).toBe('3.14')
  })

  it('3.1415 → "3.1415"（4 位小数全部保留）', () => {
    expect(formatRate(3.1415)).toBe('3.1415')
  })

  it('3.1 → "3.10"（补齐 2 位小数）', () => {
    expect(formatRate(3.1)).toBe('3.10')
  })

  it('3.14159 → "3.1416"（toFixed(4) 四舍五入）', () => {
    expect(formatRate(3.14159)).toBe('3.1416')
  })

  it('0 → "0.00"', () => {
    expect(formatRate(0)).toBe('0.00')
  })

  it('0.0001 → "0.0001"（4 位有效小数）', () => {
    expect(formatRate(0.0001)).toBe('0.0001')
  })

  it('100 → "100.00"（整数补齐 2 位小数）', () => {
    expect(formatRate(100)).toBe('100.00')
  })
})

// ---------------------------------------------------------------------------
// formatCost — 费用格式化（基于 formatRate，加 ¥ 前缀）
// ---------------------------------------------------------------------------
describe('formatCost（主进程）', () => {
  it('3.14 → "¥3.14"', () => {
    expect(formatCost(3.14)).toBe('¥3.14')
  })

  it('0 → "¥0.00"', () => {
    expect(formatCost(0)).toBe('¥0.00')
  })
})

// ---------------------------------------------------------------------------
// formatDuration — 时长格式化
// ---------------------------------------------------------------------------
describe('formatDuration（主进程）', () => {
  it('0 → "0s"', () => {
    expect(formatDuration(0)).toBe('0s')
  })

  it('30 → "30s"', () => {
    expect(formatDuration(30)).toBe('30s')
  })

  it('59 → "59s"（秒级上限）', () => {
    expect(formatDuration(59)).toBe('59s')
  })

  it('60 → "1m"', () => {
    expect(formatDuration(60)).toBe('1m')
  })

  it('90 → "1m"（Math.floor 截断）', () => {
    expect(formatDuration(90)).toBe('1m')
  })

  it('3599 → "59m"（分钟级上限）', () => {
    expect(formatDuration(3599)).toBe('59m')
  })

  it('3600 → "1h 0m"', () => {
    expect(formatDuration(3600)).toBe('1h 0m')
  })

  it('3661 → "1h 1m"', () => {
    expect(formatDuration(3661)).toBe('1h 1m')
  })

  it('86399 → "23h 59m"（小时+分钟级上限）', () => {
    expect(formatDuration(86399)).toBe('23h 59m')
  })
})

// ---------------------------------------------------------------------------
// 日期 / epoch 转换
// ---------------------------------------------------------------------------
describe('toEpochSeconds — Date → UTC 午夜 epoch', () => {
  it('2024-01-01 → 正确的 UTC 午夜秒数', () => {
    const date = new Date('2024-01-01T00:00:00Z')
    const epoch = toEpochSeconds(date)
    // 2024-01-01T00:00:00Z = 1704067200
    expect(epoch).toBe(1704067200)
  })

  it('带本地时区偏移的 Date 仍取 UTC 当天午夜', () => {
    // 无论传入什么时区时间，都取 UTC 年月日对应午夜
    const date = new Date(Date.UTC(2024, 5, 15)) // 2024-06-15 UTC
    const epoch = toEpochSeconds(date)
    const expected = Math.floor(new Date('2024-06-15T00:00:00Z').getTime() / 1000)
    expect(epoch).toBe(expected)
  })
})

describe('toExclusiveEndEpoch — Date → 次日 UTC 午夜 epoch', () => {
  it('2024-01-01 → 2024-01-02 午夜', () => {
    const date = new Date('2024-01-01T00:00:00Z')
    const end = toExclusiveEndEpoch(date)
    // 与 toEpochSeconds 相差恰好 86400 秒
    const start = toEpochSeconds(date)
    expect(end - start).toBe(86400)
    expect(end).toBe(1704153600) // 2024-01-02T00:00:00Z
  })
})

describe('epochToDateStr — Unix 秒 → YYYY-MM-DD', () => {
  it('与 toEpochSeconds 往返一致', () => {
    const date = new Date('2024-03-15T00:00:00Z')
    const epoch = toEpochSeconds(date)
    expect(epochToDateStr(epoch)).toBe('2024-03-15')
  })

  it('已知 epoch 值', () => {
    // 1704067200 = 2024-01-01T00:00:00Z
    expect(epochToDateStr(1704067200)).toBe('2024-01-01')
  })
})

describe('dateStrToEpoch — YYYY-MM-DD → Unix 秒', () => {
  it('与 epochToDateStr 往返一致', () => {
    const dateStr = '2024-07-20'
    const epoch = dateStrToEpoch(dateStr)
    expect(epochToDateStr(epoch)).toBe(dateStr)
  })

  it('已知日期的 epoch 值', () => {
    expect(dateStrToEpoch('2024-01-01')).toBe(1704067200)
  })
})

describe('epoch 往返转换一致性', () => {
  it('toEpochSeconds → epochToDateStr → dateStrToEpoch 回到原值', () => {
    const date = new Date('2025-12-31T00:00:00Z')
    const epoch = toEpochSeconds(date)
    const dateStr = epochToDateStr(epoch)
    const roundTrip = dateStrToEpoch(dateStr)
    expect(roundTrip).toBe(epoch)
  })
})
