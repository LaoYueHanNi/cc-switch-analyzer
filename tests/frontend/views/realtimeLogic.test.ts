import { describe, it, expect } from 'vitest'
import { formatTokenSpeed, formatLatency, formatTtft } from '@/utils/format'
import type { RealtimeRequestLog } from '@/types/database'

function createSampleLog(overrides: Partial<RealtimeRequestLog>): RealtimeRequestLog {
  return {
    sessionId: 'ses-1',
    model: 'claude-3-5-sonnet',
    providerId: 'CCS',
    dbType: 'CCS',
    createdAt: 1700000000,
    inputTokens: 100,
    outputTokens: 50,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    latencyMs: 1000,
    inputCost: 0.01,
    outputCost: 0.02,
    cacheReadCost: 0,
    cacheCreationCost: 0,
    totalCost: 0.03,
    ...overrides
  }
}

describe('实时页面业务逻辑', () => {
  const mockLogs: RealtimeRequestLog[] = Array.from({ length: 125 }, (_, i) =>
    createSampleLog({
      createdAt: 1700000000 + i,
      model: i % 2 === 0 ? 'claude-3-5-sonnet' : 'gpt-4o',
      dbType: i % 3 === 0 ? 'CCS' : i % 3 === 1 ? 'OpenCode' : 'Cursor',
      outputTokens: 100 + i,
      latencyMs: 2000
    })
  )

  describe('分页与切片逻辑 (PAGE_SIZE = 50)', () => {
    const PAGE_SIZE = 50

    it('计算总页数：125 条记录共 3 页', () => {
      const totalPages = Math.max(1, Math.ceil(mockLogs.length / PAGE_SIZE))
      expect(totalPages).toBe(3)
    })

    it('默认第 1 页切片 50 条', () => {
      const currentPage = 1
      const paged = mockLogs.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE)
      expect(paged).toHaveLength(50)
      expect(paged[0].outputTokens).toBe(100)
      expect(paged[49].outputTokens).toBe(149)
    })

    it('第 2 页切片 50 条', () => {
      const currentPage = 2
      const paged = mockLogs.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE)
      expect(paged).toHaveLength(50)
      expect(paged[0].outputTokens).toBe(150)
      expect(paged[49].outputTokens).toBe(199)
    })

    it('第 3 页（最后一页）切片 25 条', () => {
      const currentPage = 3
      const paged = mockLogs.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE)
      expect(paged).toHaveLength(25)
      expect(paged[0].outputTokens).toBe(200)
    })

    it('翻页按钮禁用状态边界检查（无跳页）', () => {
      const totalPages = 3

      // 第 1 页：第一页与上一页禁用，下一页可用
      let page = 1
      expect(page <= 1).toBe(true) // 第一页禁用
      expect(page <= 1).toBe(true) // 上一页禁用
      expect(page >= totalPages).toBe(false) // 下一页可用

      // 第 2 页：所有按钮可用
      page = 2
      expect(page <= 1).toBe(false)
      expect(page >= totalPages).toBe(false)

      // 第 3 页（末页）：下一页禁用
      page = 3
      expect(page <= 1).toBe(false)
      expect(page >= totalPages).toBe(true) // 下一页禁用
    })
  })

  describe('数据源过滤逻辑', () => {
    it('按数据源 canonical 名（dbType）筛选过滤记录', () => {
      const filteredCcs = mockLogs.filter(r => r.dbType === 'CCS')
      const filteredOpencode = mockLogs.filter(r => r.dbType === 'OpenCode')
      const filteredCursor = mockLogs.filter(r => r.dbType === 'Cursor')

      expect(filteredCcs.length).toBe(42)
      expect(filteredOpencode.length).toBe(42)
      expect(filteredCursor.length).toBe(41)
      expect(filteredCcs.every(r => r.dbType === 'CCS')).toBe(true)
    })

    it('未选数据源或清空时返回全量数据', () => {
      const selectedSource = ''
      const filtered = !selectedSource ? mockLogs : mockLogs.filter(r => r.dbType === selectedSource)
      expect(filtered.length).toBe(125)
    })
  })

  describe('Token 输出速度计算与格式化', () => {
    it('正常输出速度格式化：双轨展示纯吐字速率与端到端总速度', () => {
      // 100 tokens, 2000ms latency, 1000ms ttft → 纯耗时 1000ms → 100.0/50.0 tok/s
      expect(formatTokenSpeed(100, 2000, 1000)).toBe('100.0/50.0 tok/s')
      // 96 tokens, 6300ms latency, 5500ms ttft (纯吐字 800ms < 1s) → 96*1000/800 = 120.0 tok/s
      expect(formatTokenSpeed(96, 6300, 5500)).toBe('120.0/15.2 tok/s')
      // 无首字时间时纯流速显示为 -，总速度正常
      expect(formatTokenSpeed(100, 2000)).toBe('-/50.0 tok/s')
      expect(formatTokenSpeed(250, 1000)).toBe('-/250.0 tok/s')
      expect(formatTokenSpeed(75, 500)).toBe('-/150.0 tok/s')
    })

    it('首字时间等于或大于耗时时，纯流速显示为 -，总速度正常', () => {
      expect(formatTokenSpeed(100, 1000, 1000)).toBe('-/100.0 tok/s')
      expect(formatTokenSpeed(100, 1000, 1200)).toBe('-/100.0 tok/s')
    })

    it('无输出或耗时为 0 时安全返回 "-"', () => {
      expect(formatTokenSpeed(0, 1000)).toBe('-')
      expect(formatTokenSpeed(100, 0)).toBe('-')
    })
  })

  describe('首字列（TTFT timeToFirstToken）格式化', () => {
    it('有首字耗时且 > 0 时按时间规则格式化', () => {
      expect(formatTtft(350)).toBe('350ms')
      expect(formatTtft(1200)).toBe('1.2s')
    })

    it('无首字耗时（undefined、0 或负数）返回 "-"', () => {
      expect(formatTtft(undefined)).toBe('-')
      expect(formatTtft(0)).toBe('-')
      expect(formatTtft(-1)).toBe('-')
    })
  })

  describe('耗时列（latencyMs）格式化', () => {
    it('毫秒级与秒级按耗时规则格式化', () => {
      expect(formatLatency(450)).toBe('450ms')
      expect(formatLatency(1000)).toBe('1.0s')
      expect(formatLatency(2350)).toBe('2.4s')
    })
  })
})
