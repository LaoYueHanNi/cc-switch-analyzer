import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import Database from 'better-sqlite3'
import { join } from 'path'
import { tmpdir } from 'os'
import { mkdirSync, rmSync } from 'fs'
import { ExternalDbService } from '../../../electron/main/services/external-db'

const TEST_DIR = join(tmpdir(), 'cc-switch-ext-test-' + process.pid)
const DB_PATH = join(TEST_DIR, 'test-external.db')

let db: Database.Database
let service: ExternalDbService

function createTestDb(): Database.Database {
  mkdirSync(TEST_DIR, { recursive: true })
  const d = new Database(DB_PATH)
  d.exec(`
    CREATE TABLE proxy_request_logs (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      session_id TEXT,
      provider_id TEXT,
      app_type TEXT DEFAULT '',
      model TEXT NOT NULL,
      input_tokens INTEGER DEFAULT 0,
      output_tokens INTEGER DEFAULT 0,
      cache_read_tokens INTEGER DEFAULT 0,
      cache_creation_tokens INTEGER DEFAULT 0,
      latency_ms INTEGER DEFAULT 0,
      status_code INTEGER DEFAULT 200,
      created_at INTEGER NOT NULL
    );
    CREATE TABLE providers (
      id TEXT,
      app_type TEXT DEFAULT '',
      name TEXT,
      PRIMARY KEY (id, app_type)
    );
  `)

  d.exec(`INSERT INTO providers (id, app_type, name) VALUES ('anthropic', 'claude', 'Anthropic')`)
  d.exec(`INSERT INTO providers (id, app_type, name) VALUES ('openrouter', 'openai', 'OpenRouter')`)

  const insert = d.prepare(`
    INSERT INTO proxy_request_logs
      (session_id, provider_id, app_type, model, input_tokens, output_tokens,
       cache_read_tokens, cache_creation_tokens, latency_ms, status_code, created_at)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
  `)

  // app_type 必须与 providers 表匹配，JOIN 才能取到 name

  // epoch 1704067200 = 2024-01-01 00:00:00 UTC
  insert.run('sess-1', 'anthropic', 'claude', 'claude-sonnet-4', 1000, 500, 200, 100, 150, 200, 1704067200)
  insert.run('sess-1', 'anthropic', 'claude', 'claude-sonnet-4', 2000, 1000, 400, 200, 200, 200, 1704120000)
  insert.run('sess-1', 'openrouter', 'openai', 'claude-haiku-4', 500, 250, 100, 50, 80, 200, 1704140000)
  insert.run('sess-2', 'anthropic', 'claude', 'claude-sonnet-4', 3000, 1500, 600, 300, 250, 200, 1704200000)
  insert.run('sess-2', 'openrouter', 'openai', 'claude-haiku-4', 800, 400, 200, 100, 100, 200, 1704260000)
  insert.run('sess-2', 'anthropic', 'claude', 'claude-sonnet-4', 100, 50, 0, 0, 300, 500, 1704300000)
  insert.run('sess-3', 'anthropic', 'claude', 'claude-sonnet-4', 500, 250, 100, 50, 120, 200, 1704153600)

  return d
}

function cleanup() {
  try { service?.close() } catch { /* ok */ }
  try { db?.close() } catch { /* ok */ }
  try { rmSync(TEST_DIR, { recursive: true, force: true }) } catch { /* ok */ }
}

beforeEach(() => {
  db = createTestDb()
  service = new ExternalDbService()
  ;(service as any).db = db
  ;(service as any).dbPath = DB_PATH
})

afterEach(cleanup)

const EMPTY_FILTER = { fromDate: null, toDate: null, providerId: '', modelId: '' }

describe('ExternalDbService — getProviders', () => {
  it('返回去重的供应商列表', () => {
    const providers = service.getProviders()
    expect(providers.length).toBe(2)
    const names = providers.map(p => p.name)
    expect(names).toContain('Anthropic')
    expect(names).toContain('OpenRouter')
  })
})

describe('ExternalDbService — getModels', () => {
  it('返回去重的模型列表（排序）', () => {
    const models = service.getModels()
    expect(models).toEqual(['claude-haiku-4', 'claude-sonnet-4'])
  })
})

describe('ExternalDbService — getSummary', () => {
  it('无筛选 → 正确汇总', () => {
    const s = service.getSummary(EMPTY_FILTER)
    expect(s.totalRequests).toBe(7)
    expect(s.successCount).toBe(6)
    expect(s.totalInput).toBe(7900)
    expect(s.totalOutput).toBe(3950)
  })

  it('按模型筛选', () => {
    const s = service.getSummary({ ...EMPTY_FILTER, modelId: 'claude-haiku-4' })
    expect(s.totalRequests).toBe(2)
    expect(s.totalInput).toBe(1300)
  })
})

describe('ExternalDbService — getModelBreakdown', () => {
  it('按模型分组，按请求数降序', () => {
    const breakdown = service.getModelBreakdown(EMPTY_FILTER)
    expect(breakdown[0].model).toBe('claude-sonnet-4')
    expect(breakdown[1].model).toBe('claude-haiku-4')
    expect(breakdown[0].requests).toBe(5)
    expect(breakdown[0].inputTokens).toBe(6600)
  })
})

describe('ExternalDbService — getProviderBreakdown', () => {
  it('包含成功率和平均延迟', () => {
    const breakdown = service.getProviderBreakdown(EMPTY_FILTER)
    expect(breakdown.length).toBe(2)
    const anthropic = breakdown.find(p => p.providerId === 'anthropic')!
    expect(anthropic.requests).toBe(5)
    expect(anthropic.successes).toBe(4)
  })
})

describe('ExternalDbService — getDailyTrend', () => {
  it('按天+模型分组', () => {
    const trend = service.getDailyTrend(EMPTY_FILTER)
    expect(trend.length).toBeGreaterThanOrEqual(3)
    const day1 = trend.filter(r => r.day === '2024-01-01')
    expect(day1.length).toBeGreaterThanOrEqual(2)
  })
})

describe('ExternalDbService — getSessionBreakdown', () => {
  it('按请求数降序', () => {
    const sessions = service.getSessionBreakdown(EMPTY_FILTER)
    expect(sessions.length).toBe(3)
    expect(sessions[0].sessionId).toBe('sess-2')
  })
})

describe('ExternalDbService — getSessionTimestamps', () => {
  it('空输入 → 空 Map', () => {
    expect(service.getSessionTimestamps([]).size).toBe(0)
  })

  it('返回按时间排序的时间戳', () => {
    const map = service.getSessionTimestamps(['sess-1'])
    const ts = map.get('sess-1')!
    expect(ts.length).toBe(3)
    expect(ts[0]).toBeLessThanOrEqual(ts[ts.length - 1])
  })
})

describe('ExternalDbService — 基础查询', () => {
  it('getRecordCount', () => {
    expect(service.getRecordCount()).toBe(7)
  })

  it('getDateRange', () => {
    const range = service.getDateRange()
    expect(range.min).toBeLessThanOrEqual(range.max)
  })
})
