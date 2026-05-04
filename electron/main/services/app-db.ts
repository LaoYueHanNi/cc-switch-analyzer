import Database from 'better-sqlite3'
import { mkdirSync, existsSync } from 'fs'
import { dirname } from 'path'
import { APP_DB_PATH, DEFAULT_EXCHANGE_RATE } from '../utils/constants'

// 定价覆盖
export interface PricingOverride {
  modelId: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  updatedAt: number
}

// 时间定价规则
export interface TimePricingRule {
  id: number
  modelId: string
  startTime: number
  endTime: number
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  label: string
}

// 应用自有数据库服务
export class AppDbService {
  private db: Database.Database

  constructor() {
    // 确保目录存在
    const dir = dirname(APP_DB_PATH)
    if (!existsSync(dir)) {
      mkdirSync(dir, { recursive: true })
    }

    this.db = new Database(APP_DB_PATH)
    // 启用 WAL 模式
    this.db.pragma('journal_mode = WAL')
    this.initSchema()
  }

  private initSchema(): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS pricing_overrides (
        model_id TEXT PRIMARY KEY,
        input_cost_per_million REAL NOT NULL,
        output_cost_per_million REAL NOT NULL,
        cache_read_cost_per_million REAL NOT NULL,
        cache_creation_cost_per_million REAL NOT NULL,
        updated_at INTEGER DEFAULT (strftime('%s','now'))
      );

      CREATE TABLE IF NOT EXISTS time_pricing_overrides (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        model_id TEXT NOT NULL,
        start_time INTEGER NOT NULL,
        end_time INTEGER NOT NULL,
        input_cost_per_million REAL NOT NULL,
        output_cost_per_million REAL NOT NULL,
        cache_read_cost_per_million REAL NOT NULL,
        cache_creation_cost_per_million REAL NOT NULL,
        label TEXT DEFAULT ''
      );

      CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL,
        updated_at INTEGER DEFAULT (strftime('%s','now'))
      );

      CREATE TABLE IF NOT EXISTS session_titles (
        session_id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        created_at INTEGER DEFAULT (strftime('%s','now'))
      );
    `)
  }

  close(): void {
    this.db.close()
  }

  // ========== 设置管理 ==========

  getSetting(key: string): string | null {
    const row = this.db.prepare('SELECT value FROM settings WHERE key = ?').get(key) as { value: string } | undefined
    return row ? row.value : null
  }

  setSetting(key: string, value: string): void {
    this.db.prepare(`
      INSERT OR REPLACE INTO settings (key, value, updated_at)
      VALUES (?, ?, strftime('%s','now'))
    `).run(key, value)
  }

  // 汇率（便捷方法）
  getExchangeRate(): number {
    const v = this.getSetting('exchange_rate')
    return v ? parseFloat(v) : DEFAULT_EXCHANGE_RATE
  }

  setExchangeRate(rate: number): void {
    this.setSetting('exchange_rate', String(rate))
  }

  // ========== 定价覆盖 CRUD ==========

  getAllOverrides(): PricingOverride[] {
    return this.db.prepare('SELECT * FROM pricing_overrides ORDER BY model_id').all().map((r: any) => ({
      modelId: r.model_id,
      inputCostPerMillion: r.input_cost_per_million,
      outputCostPerMillion: r.output_cost_per_million,
      cacheReadCostPerMillion: r.cache_read_cost_per_million,
      cacheCreationCostPerMillion: r.cache_creation_cost_per_million,
      updatedAt: r.updated_at
    }))
  }

  saveOverride(
    modelId: string,
    inputCost: number,
    outputCost: number,
    cacheReadCost: number,
    cacheCreationCost: number
  ): void {
    this.db.prepare(`
      INSERT OR REPLACE INTO pricing_overrides
        (model_id, input_cost_per_million, output_cost_per_million,
         cache_read_cost_per_million, cache_creation_cost_per_million, updated_at)
      VALUES (?, ?, ?, ?, ?, strftime('%s','now'))
    `).run(modelId, inputCost, outputCost, cacheReadCost, cacheCreationCost)
  }

  deleteOverride(modelId: string): void {
    this.db.prepare('DELETE FROM pricing_overrides WHERE model_id = ?').run(modelId)
  }

  // ========== 时间定价 CRUD ==========

  getAllTimeOverrides(): TimePricingRule[] {
    return this.db.prepare('SELECT * FROM time_pricing_overrides ORDER BY model_id, start_time').all().map((r: any) => ({
      id: r.id,
      modelId: r.model_id,
      startTime: r.start_time,
      endTime: r.end_time,
      inputCostPerMillion: r.input_cost_per_million,
      outputCostPerMillion: r.output_cost_per_million,
      cacheReadCostPerMillion: r.cache_read_cost_per_million,
      cacheCreationCostPerMillion: r.cache_creation_cost_per_million,
      label: r.label
    }))
  }

  addTimeOverride(
    modelId: string,
    startTime: number,
    endTime: number,
    inputCost: number,
    outputCost: number,
    cacheReadCost: number,
    cacheCreationCost: number,
    label: string
  ): number {
    const result = this.db.prepare(`
      INSERT INTO time_pricing_overrides
        (model_id, start_time, end_time, input_cost_per_million, output_cost_per_million,
         cache_read_cost_per_million, cache_creation_cost_per_million, label)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    `).run(modelId, startTime, endTime, inputCost, outputCost, cacheReadCost, cacheCreationCost, label || '')
    return Number(result.lastInsertRowid)
  }

  updateTimeOverride(
    id: number,
    startTime: number,
    endTime: number,
    inputCost: number,
    outputCost: number,
    cacheReadCost: number,
    cacheCreationCost: number,
    label: string
  ): void {
    this.db.prepare(`
      UPDATE time_pricing_overrides SET
        start_time = ?, end_time = ?,
        input_cost_per_million = ?, output_cost_per_million = ?,
        cache_read_cost_per_million = ?, cache_creation_cost_per_million = ?,
        label = ?
      WHERE id = ?
    `).run(startTime, endTime, inputCost, outputCost, cacheReadCost, cacheCreationCost, label || '', id)
  }

  deleteTimeOverride(id: number): void {
    this.db.prepare('DELETE FROM time_pricing_overrides WHERE id = ?').run(id)
  }

  // ========== 会话标题 ==========

  getSessionTitles(sessionIds: string[]): Map<string, string> {
    if (sessionIds.length === 0) return new Map()
    const placeholders = sessionIds.map(() => '?').join(',')
    const rows = this.db.prepare(
      `SELECT session_id, title FROM session_titles WHERE session_id IN (${placeholders})`
    ).all(...sessionIds) as { session_id: string; title: string }[]
    const result = new Map<string, string>()
    for (const row of rows) { result.set(row.session_id, row.title) }
    return result
  }

  saveSessionTitle(sessionId: string, title: string): void {
    this.db.prepare(
      'INSERT OR REPLACE INTO session_titles (session_id, title, created_at) VALUES (?, ?, strftime(\'%s\',\'now\'))'
    ).run(sessionId, title)
  }
}
