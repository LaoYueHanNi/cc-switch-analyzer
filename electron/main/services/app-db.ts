import Database from 'better-sqlite3'
import { mkdirSync, existsSync, copyFileSync } from 'fs'
import { dirname } from 'path'
import { APP_DB_PATH, DEFAULT_EXCHANGE_RATE } from '../utils/constants'

// 上下文定价档位
export interface ContextTier {
  id?: number
  threshold: number
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
}

// 定价覆盖
export interface PricingOverride {
  modelId: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  updatedAt: number
  contextTiers: ContextTier[]
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
  contextTiers: ContextTier[]
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

    const version = this.getSchemaVersion()

    if (version < 1) {
      this.backupBeforeMigration(version)
      this.migrateV1()
    }
    // 未来: if (version < 2) { this.backupBeforeMigration(1); this.migrateV2(); }
  }

  private getSchemaVersion(): number {
    const row = this.db.prepare("SELECT value FROM settings WHERE key = 'schema_version'").get() as { value: string } | undefined
    return row ? parseInt(row.value, 10) || 0 : 0
  }

  private setSchemaVersion(version: number): void {
    this.db.prepare(
      "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('schema_version', ?, strftime('%s','now'))"
    ).run(String(version))
  }

  private backupBeforeMigration(fromVersion: number): void {
    const backupPath = `${APP_DB_PATH}.v${fromVersion}.bak`
    if (!existsSync(backupPath)) {
      copyFileSync(APP_DB_PATH, backupPath)
    }
  }

  private migrateV1(): void {
    // pricing_overrides: 复合 PK (model_id, threshold)
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS pricing_overrides (
        model_id TEXT NOT NULL,
        threshold INTEGER NOT NULL DEFAULT 0,
        input_cost_per_million REAL NOT NULL,
        output_cost_per_million REAL NOT NULL,
        cache_read_cost_per_million REAL NOT NULL,
        cache_creation_cost_per_million REAL NOT NULL,
        updated_at INTEGER DEFAULT (strftime('%s','now')),
        PRIMARY KEY (model_id, threshold)
      );
    `)

    // 检查旧表结构（无 threshold 列），需要迁移
    const hasThreshold = this.db.prepare('SELECT threshold FROM pricing_overrides LIMIT 0').safeIntegers().get()
    if (hasThreshold === undefined && !this.db.pragma('table_info(pricing_overrides)').some((c: any) => c.name === 'threshold')) {
      this.db.exec(`
        CREATE TABLE pricing_overrides_new (
          model_id TEXT NOT NULL,
          threshold INTEGER NOT NULL DEFAULT 0,
          input_cost_per_million REAL NOT NULL,
          output_cost_per_million REAL NOT NULL,
          cache_read_cost_per_million REAL NOT NULL,
          cache_creation_cost_per_million REAL NOT NULL,
          updated_at INTEGER DEFAULT (strftime('%s','now')),
          PRIMARY KEY (model_id, threshold)
        );
        INSERT INTO pricing_overrides_new
          (model_id, threshold, input_cost_per_million, output_cost_per_million,
           cache_read_cost_per_million, cache_creation_cost_per_million, updated_at)
        SELECT model_id, 0, input_cost_per_million, output_cost_per_million,
               cache_read_cost_per_million, cache_creation_cost_per_million, updated_at
        FROM pricing_overrides;
        DROP TABLE pricing_overrides;
        ALTER TABLE pricing_overrides_new RENAME TO pricing_overrides;
      `)
    }

    // time_pricing_overrides: 添加 threshold 列
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS time_pricing_overrides (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        model_id TEXT NOT NULL,
        start_time INTEGER NOT NULL,
        end_time INTEGER NOT NULL,
        input_cost_per_million REAL NOT NULL,
        output_cost_per_million REAL NOT NULL,
        cache_read_cost_per_million REAL NOT NULL,
        cache_creation_cost_per_million REAL NOT NULL,
        label TEXT DEFAULT '',
        threshold INTEGER NOT NULL DEFAULT 0
      );
    `)

    const columns = this.db.pragma('table_info(time_pricing_overrides)') as { name: string }[]
    if (!columns.some(c => c.name === 'threshold')) {
      this.db.exec('ALTER TABLE time_pricing_overrides ADD COLUMN threshold INTEGER NOT NULL DEFAULT 0')
    }

    this.setSchemaVersion(1)
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
    const rows = this.db.prepare('SELECT * FROM pricing_overrides ORDER BY model_id, threshold').all() as any[]
    const map = new Map<string, PricingOverride>()
    for (const r of rows) {
      if (!map.has(r.model_id)) {
        map.set(r.model_id, {
          modelId: r.model_id,
          inputCostPerMillion: r.input_cost_per_million,
          outputCostPerMillion: r.output_cost_per_million,
          cacheReadCostPerMillion: r.cache_read_cost_per_million,
          cacheCreationCostPerMillion: r.cache_creation_cost_per_million,
          updatedAt: r.updated_at,
          contextTiers: []
        })
      }
      if (r.threshold > 0) {
        map.get(r.model_id)!.contextTiers.push({
          threshold: r.threshold,
          inputCostPerMillion: r.input_cost_per_million,
          outputCostPerMillion: r.output_cost_per_million,
          cacheReadCostPerMillion: r.cache_read_cost_per_million,
          cacheCreationCostPerMillion: r.cache_creation_cost_per_million
        })
      }
    }
    return Array.from(map.values()).sort((a, b) => a.modelId.localeCompare(b.modelId))
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
        (model_id, threshold, input_cost_per_million, output_cost_per_million,
         cache_read_cost_per_million, cache_creation_cost_per_million, updated_at)
      VALUES (?, 0, ?, ?, ?, ?, strftime('%s','now'))
    `).run(modelId, inputCost, outputCost, cacheReadCost, cacheCreationCost)
  }

  saveOverrideTier(
    modelId: string,
    threshold: number,
    inputCost: number,
    outputCost: number,
    cacheReadCost: number,
    cacheCreationCost: number
  ): void {
    this.db.prepare(`
      INSERT OR REPLACE INTO pricing_overrides
        (model_id, threshold, input_cost_per_million, output_cost_per_million,
         cache_read_cost_per_million, cache_creation_cost_per_million, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, strftime('%s','now'))
    `).run(modelId, threshold, inputCost, outputCost, cacheReadCost, cacheCreationCost)
  }

  deleteOverrideTier(modelId: string, threshold: number): void {
    this.db.prepare('DELETE FROM pricing_overrides WHERE model_id = ? AND threshold = ?').run(modelId, threshold)
  }

  deleteOverride(modelId: string): void {
    this.db.prepare('DELETE FROM pricing_overrides WHERE model_id = ?').run(modelId)
  }

  // ========== 时间定价 CRUD ==========

  getAllTimeOverrides(): TimePricingRule[] {
    const rows = this.db.prepare('SELECT * FROM time_pricing_overrides ORDER BY model_id, start_time, threshold').all() as any[]
    const map = new Map<string, TimePricingRule>()
    for (const r of rows) {
      const key = `${r.model_id}:${r.start_time}:${r.end_time}`
      if (!map.has(key)) {
        map.set(key, {
          id: r.id,
          modelId: r.model_id,
          startTime: r.start_time,
          endTime: r.end_time,
          inputCostPerMillion: r.input_cost_per_million,
          outputCostPerMillion: r.output_cost_per_million,
          cacheReadCostPerMillion: r.cache_read_cost_per_million,
          cacheCreationCostPerMillion: r.cache_creation_cost_per_million,
          label: r.label,
          contextTiers: []
        })
      }
      if (r.threshold > 0) {
        map.get(key)!.contextTiers.push({
          id: r.id,
          threshold: r.threshold,
          inputCostPerMillion: r.input_cost_per_million,
          outputCostPerMillion: r.output_cost_per_million,
          cacheReadCostPerMillion: r.cache_read_cost_per_million,
          cacheCreationCostPerMillion: r.cache_creation_cost_per_million
        })
      }
    }
    return Array.from(map.values()).sort((a, b) => a.modelId.localeCompare(b.modelId) || a.startTime - b.startTime)
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
         cache_read_cost_per_million, cache_creation_cost_per_million, label, threshold)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)
    `).run(modelId, startTime, endTime, inputCost, outputCost, cacheReadCost, cacheCreationCost, label || '')
    return Number(result.lastInsertRowid)
  }

  addTimeOverrideTier(
    modelId: string,
    startTime: number,
    endTime: number,
    threshold: number,
    inputCost: number,
    outputCost: number,
    cacheReadCost: number,
    cacheCreationCost: number
  ): number {
    const result = this.db.prepare(`
      INSERT INTO time_pricing_overrides
        (model_id, start_time, end_time, input_cost_per_million, output_cost_per_million,
         cache_read_cost_per_million, cache_creation_cost_per_million, label, threshold)
      SELECT ?, ?, ?, ?, ?, ?, ?, label, ?
      FROM time_pricing_overrides
      WHERE model_id = ? AND start_time = ? AND end_time = ? AND threshold = 0
      LIMIT 1
    `).run(modelId, startTime, endTime, inputCost, outputCost, cacheReadCost, cacheCreationCost, threshold, modelId, startTime, endTime)
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

  updateTimeOverrideRange(
    modelId: string,
    oldStart: number,
    oldEnd: number,
    newStart: number,
    newEnd: number,
    label: string
  ): void {
    this.db.prepare(`
      UPDATE time_pricing_overrides SET
        start_time = ?, end_time = ?, label = ?
      WHERE model_id = ? AND start_time = ? AND end_time = ?
    `).run(newStart, newEnd, label || '', modelId, oldStart, oldEnd)
  }

  updateTimeOverrideTier(
    id: number,
    inputCost: number,
    outputCost: number,
    cacheReadCost: number,
    cacheCreationCost: number
  ): void {
    this.db.prepare(`
      UPDATE time_pricing_overrides SET
        input_cost_per_million = ?, output_cost_per_million = ?,
        cache_read_cost_per_million = ?, cache_creation_cost_per_million = ?
      WHERE id = ?
    `).run(inputCost, outputCost, cacheReadCost, cacheCreationCost, id)
  }

  deleteTimeOverride(id: number): void {
    this.db.prepare('DELETE FROM time_pricing_overrides WHERE id = ?').run(id)
  }

  deleteTimeOverrideGroup(modelId: string, startTime: number, endTime: number): void {
    this.db.prepare('DELETE FROM time_pricing_overrides WHERE model_id = ? AND start_time = ? AND end_time = ?').run(modelId, startTime, endTime)
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
