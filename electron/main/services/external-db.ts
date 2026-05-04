import Database from 'better-sqlite3'
import type { Statement } from 'better-sqlite3'
import { toEpochSeconds, toExclusiveEndEpoch } from '../utils/format'
import { CACHE_WINDOW_DAYS, SESSION_TOP_N, REALTIME_WINDOW_SEC } from '../utils/constants'

// 筛选参数类型
export interface FilterParams {
  fromDate: Date | null
  toDate: Date | null
  providerId: string  // 空字符串 = 全部
  modelId: string     // 空字符串 = 全部
}

// 查询结果类型
export interface Provider {
  id: string
  name: string
}

export interface ModelPricing {
  modelId: string
  displayName: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
}

export interface SummaryData {
  totalRequests: number
  successCount: number
  totalInput: number
  totalOutput: number
  totalCacheRead: number
  totalCacheCreation: number
  avgLatency: number
}

export interface ModelBreakdown {
  model: string
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface ProviderBreakdown {
  providerName: string
  providerId: string
  requests: number
  successes: number
  successRate: number
  avgLatency: number
}

export interface ProviderModelToken {
  providerId: string
  model: string
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface DailyTrendRow {
  day: string
  model: string
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
  avgLatency: number
}

export interface RealtimeBucket {
  bucket: number
  requests: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface RealtimeRequestLog {
  model: string
  providerId: string
  createdAt: number
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheCreationTokens: number
  latencyMs: number
  inputCost: number
  outputCost: number
  cacheReadCost: number
  cacheCreationCost: number
  totalCost: number
}

export interface SessionBreakdown {
  sessionId: string
  requests: number
  maxContextWidth: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
  firstAt: number
  lastAt: number
}

export interface SessionModelToken {
  sessionId: string
  model: string
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface SessionRequestToken {
  sessionId: string
  model: string
  createdAt: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheCreation: number
}

export interface CacheWindow {
  startTs: number
  endTs: number
  durationSec: number
  hits: number
}

// 外部 CC-Switch 数据库服务（只读）
export class ExternalDbService {
  private db: Database.Database | null = null
  private dbPath: string = ''

  // 打开外部数据库（只读模式，不执行任何写入操作）
  open(filePath: string): void {
    this.close()
    this.dbPath = filePath
    this.db = new Database(filePath, { readonly: true })
  }

  // 关闭连接
  close(): void {
    if (this.db) {
      this.db.close()
      this.db = null
    }
  }

  get isOpen(): boolean {
    return this.db !== null && this.db.open
  }

  get path(): string {
    return this.dbPath
  }

  // 获取数据库实例（内部使用）
  private getDb(): Database.Database {
    if (!this.db) throw new Error('数据库未打开')
    return this.db
  }

  // 获取总记录数
  getRecordCount(): number {
    const row = this.getDb().prepare('SELECT COUNT(*) AS count FROM proxy_request_logs').get() as { count: number }
    return row.count
  }

  // 获取最新时间戳（用于刷新检测）
  getLatestTimestamp(): number | null {
    const row = this.getDb().prepare('SELECT MAX(created_at) AS m FROM proxy_request_logs').get() as { m: number | null }
    return row.m
  }

  // ========== FilterParams 动态构建 ==========

  private buildWhereClause(params: FilterParams, aliased: boolean): { sql: string; binds: any[] } {
    const prefix = aliased ? 'l.' : ''
    const clauses: string[] = ['1=1']
    const binds: any[] = []

    if (params.fromDate) {
      clauses.push(`${prefix}created_at >= ?`)
      binds.push(toEpochSeconds(params.fromDate))
    }
    if (params.toDate) {
      clauses.push(`${prefix}created_at < ?`)  // to 是 exclusive，+1 天
      binds.push(toExclusiveEndEpoch(params.toDate))
    }
    if (params.providerId) {
      clauses.push(`${prefix}provider_id = ?`)
      binds.push(params.providerId)
    }
    if (params.modelId) {
      clauses.push(`${prefix}model = ?`)
      binds.push(params.modelId)
    }

    return { sql: `WHERE ${clauses.join(' AND ')}`, binds }
  }

  // ========== 基础查询（无筛选） ==========

  getProviders(): Provider[] {
    return this.getDb().prepare(`
      SELECT DISTINCT l.provider_id AS id, COALESCE(p.name, l.provider_id) AS name
      FROM proxy_request_logs l
      LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
      ORDER BY name
    `).all() as Provider[]
  }

  getModels(): string[] {
    const rows = this.getDb().prepare('SELECT DISTINCT model FROM proxy_request_logs ORDER BY model').all() as { model: string }[]
    return rows.map(r => r.model)
  }

  getDateRange(): { min: number; max: number } {
    const row = this.getDb().prepare('SELECT MIN(created_at) AS min, MAX(created_at) AS max FROM proxy_request_logs').get() as { min: number; max: number }
    return { min: row.min, max: row.max }
  }

  getBasePricing(): ModelPricing[] {
    return this.getDb().prepare('SELECT * FROM model_pricing ORDER BY model_id').all().map((r: any) => ({
      modelId: r.model_id,
      displayName: r.display_name || r.model_id,
      inputCostPerMillion: r.input_cost_per_million,
      outputCostPerMillion: r.output_cost_per_million,
      cacheReadCostPerMillion: r.cache_read_cost_per_million,
      cacheCreationCostPerMillion: r.cache_creation_cost_per_million
    })) as ModelPricing[]
  }

  // ========== 筛选查询 ==========

  // 摘要统计
  getSummary(params: FilterParams): SummaryData {
    const { sql, binds } = this.buildWhereClause(params, false)
    const row = this.getDb().prepare(`
      SELECT
        COUNT(*) AS totalRequests,
        SUM(CASE WHEN status_code=200 THEN 1 ELSE 0 END) AS successCount,
        SUM(input_tokens) AS totalInput,
        SUM(output_tokens) AS totalOutput,
        SUM(cache_read_tokens) AS totalCacheRead,
        SUM(cache_creation_tokens) AS totalCacheCreation,
        ROUND(AVG(latency_ms), 0) AS avgLatency
      FROM proxy_request_logs
      ${sql}
    `).get(...binds) as any
    return {
      totalRequests: row.totalRequests || 0,
      successCount: row.successCount || 0,
      totalInput: row.totalInput || 0,
      totalOutput: row.totalOutput || 0,
      totalCacheRead: row.totalCacheRead || 0,
      totalCacheCreation: row.totalCacheCreation || 0,
      avgLatency: row.avgLatency || 0
    }
  }

  // 模型维度统计
  getModelBreakdown(params: FilterParams): ModelBreakdown[] {
    const { sql, binds } = this.buildWhereClause(params, true)
    return this.getDb().prepare(`
      SELECT
        l.model,
        COUNT(*) AS requests,
        SUM(l.input_tokens) AS inputTokens,
        SUM(l.output_tokens) AS outputTokens,
        SUM(l.cache_read_tokens) AS cacheRead,
        SUM(l.cache_creation_tokens) AS cacheCreation
      FROM proxy_request_logs l
      ${sql}
      GROUP BY l.model
      ORDER BY requests DESC
    `).all(...binds) as ModelBreakdown[]
  }

  // 供应商维度统计
  getProviderBreakdown(params: FilterParams): ProviderBreakdown[] {
    const { sql, binds } = this.buildWhereClause(params, true)
    return this.getDb().prepare(`
      SELECT
        COALESCE(p.name, l.provider_id) AS providerName,
        l.provider_id AS providerId,
        COUNT(*) AS requests,
        SUM(CASE WHEN l.status_code=200 THEN 1 ELSE 0 END) AS successes,
        ROUND(100.0 * SUM(CASE WHEN l.status_code=200 THEN 1 ELSE 0 END) / COUNT(*), 1) AS successRate,
        ROUND(AVG(l.latency_ms), 0) AS avgLatency
      FROM proxy_request_logs l
      LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
      ${sql}
      GROUP BY l.provider_id
      ORDER BY requests DESC
    `).all(...binds) as ProviderBreakdown[]
  }

  // 供应商-模型 Token 映射
  getProviderModelTokens(params: FilterParams): ProviderModelToken[] {
    const { sql, binds } = this.buildWhereClause(params, true)
    return this.getDb().prepare(`
      SELECT
        l.provider_id AS providerId,
        l.model,
        SUM(l.input_tokens) AS inputTokens,
        SUM(l.output_tokens) AS outputTokens,
        SUM(l.cache_read_tokens) AS cacheRead,
        SUM(l.cache_creation_tokens) AS cacheCreation
      FROM proxy_request_logs l
      ${sql}
      GROUP BY l.provider_id, l.model
    `).all(...binds) as ProviderModelToken[]
  }

  // 每日趋势
  getDailyTrend(params: FilterParams): DailyTrendRow[] {
    const { sql, binds } = this.buildWhereClause(params, true)
    return this.getDb().prepare(`
      SELECT
        date(l.created_at, 'unixepoch') AS day,
        l.model,
        COUNT(*) AS requests,
        SUM(l.input_tokens) AS inputTokens,
        SUM(l.output_tokens) AS outputTokens,
        SUM(l.cache_read_tokens) AS cacheRead,
        SUM(l.cache_creation_tokens) AS cacheCreation,
        ROUND(AVG(l.latency_ms), 0) AS avgLatency
      FROM proxy_request_logs l
      ${sql}
      GROUP BY day, l.model
      ORDER BY day
    `).all(...binds) as DailyTrendRow[]
  }

  // 各模型平均缓存时长（复杂 CTE 查询）
  getCacheNonDecayDuration(params: FilterParams): Map<string, number> {
    const thirtyDaysAgo = Math.floor(Date.now() / 1000) - CACHE_WINDOW_DAYS * 86400
    const { sql, binds } = this.buildWhereClause(params, true)

    const rows = this.getDb().prepare(`
      WITH marked AS (
        SELECT l.session_id, l.model, l.created_at, l.cache_read_tokens,
          CASE WHEN l.cache_read_tokens = 0 THEN 1 ELSE 0 END AS window_break
        FROM proxy_request_logs l
        ${sql} AND l.created_at >= ?
      ),
      grouped AS (
        SELECT session_id, model, created_at, cache_read_tokens,
          SUM(window_break) OVER (
            PARTITION BY session_id, model ORDER BY created_at
          ) AS grp
        FROM marked
      ),
      window_durations AS (
        SELECT model, grp,
          MAX(created_at) - MIN(created_at) AS duration_sec,
          MAX(created_at) AS end_ts
        FROM grouped
        WHERE cache_read_tokens > 0
        GROUP BY session_id, model, grp
        HAVING COUNT(*) > 1
      ),
      ranked AS (
        SELECT model, duration_sec,
          ROW_NUMBER() OVER (PARTITION BY model ORDER BY end_ts DESC) AS rn
        FROM window_durations
      )
      SELECT model, AVG(duration_sec) AS avg_duration_sec
      FROM ranked WHERE rn <= 10 GROUP BY model
    `).all(...binds, thirtyDaysAgo) as { model: string; avg_duration_sec: number }[]

    const result = new Map<string, number>()
    for (const row of rows) {
      result.set(row.model, Math.round(row.avg_duration_sec))
    }
    return result
  }

  // 单模型缓存窗口详情
  getRecentCacheWindows(modelId: string): CacheWindow[] {
    const thirtyDaysAgo = Math.floor(Date.now() / 1000) - CACHE_WINDOW_DAYS * 86400
    return this.getDb().prepare(`
      WITH marked AS (
        SELECT l.session_id, l.created_at, l.cache_read_tokens,
          CASE WHEN l.cache_read_tokens = 0 THEN 1 ELSE 0 END AS window_break
        FROM proxy_request_logs l
        WHERE l.model = ? AND l.created_at >= ?
      ),
      grouped AS (
        SELECT session_id, created_at, cache_read_tokens,
          SUM(window_break) OVER (
            PARTITION BY session_id ORDER BY created_at
          ) AS grp
        FROM marked
      ),
      window_durations AS (
        SELECT session_id, grp,
          MIN(created_at) AS start_ts,
          MAX(created_at) AS end_ts,
          MAX(created_at) - MIN(created_at) AS duration_sec,
          COUNT(*) AS hits
        FROM grouped
        WHERE cache_read_tokens > 0
        GROUP BY session_id, grp
        HAVING COUNT(*) > 1
      )
      SELECT start_ts, end_ts, duration_sec, hits
      FROM window_durations
      ORDER BY end_ts DESC
      LIMIT 10
    `).all(modelId, thirtyDaysAgo) as CacheWindow[]
  }

  // 会话统计（Top 50）
  getSessionBreakdown(params: FilterParams): SessionBreakdown[] {
    const { sql, binds } = this.buildWhereClause(params, true)
    return this.getDb().prepare(`
      SELECT
        l.session_id AS sessionId,
        COUNT(*) AS requests,
        (SELECT l2.input_tokens + l2.cache_read_tokens
         FROM proxy_request_logs l2
         WHERE l2.session_id = l.session_id
           AND l2.session_id IS NOT NULL AND l2.session_id != ''
           AND l2.input_tokens + l2.cache_read_tokens > 0
         ORDER BY l2.created_at DESC LIMIT 1) AS maxContextWidth,
        SUM(l.input_tokens) AS inputTokens,
        SUM(l.output_tokens) AS outputTokens,
        SUM(l.cache_read_tokens) AS cacheRead,
        SUM(l.cache_creation_tokens) AS cacheCreation,
        MIN(l.created_at) AS firstAt,
        MAX(l.created_at) AS lastAt
      FROM proxy_request_logs l
      ${sql}
        AND l.session_id IS NOT NULL AND l.session_id != ''
      GROUP BY l.session_id
      ORDER BY requests DESC
      LIMIT ${SESSION_TOP_N}
    `).all(...binds) as SessionBreakdown[]
  }

  // 会话-模型 Token 分解
  getSessionModelTokens(params: FilterParams): SessionModelToken[] {
    const { sql, binds } = this.buildWhereClause(params, true)
    const { sql: subSql, binds: subBinds } = this.buildWhereClause(params, false)

    return this.getDb().prepare(`
      SELECT
        l.session_id AS sessionId,
        l.model,
        SUM(l.input_tokens) AS inputTokens,
        SUM(l.output_tokens) AS outputTokens,
        SUM(l.cache_read_tokens) AS cacheRead,
        SUM(l.cache_creation_tokens) AS cacheCreation
      FROM proxy_request_logs l
      ${sql}
        AND l.session_id IS NOT NULL AND l.session_id != ''
        AND l.session_id IN (
          SELECT s.session_id FROM proxy_request_logs s
          ${subSql}
            AND s.session_id IS NOT NULL AND s.session_id != ''
          GROUP BY s.session_id ORDER BY COUNT(*) DESC LIMIT ${SESSION_TOP_N}
        )
      GROUP BY l.session_id, l.model
    `).all(...binds, ...subBinds) as SessionModelToken[]
  }

  // 会话-请求级 Token 数据（用于时间感知定价）
  getSessionRequestTokens(params: FilterParams): SessionRequestToken[] {
    const { sql, binds } = this.buildWhereClause(params, true)
    const { sql: subSql, binds: subBinds } = this.buildWhereClause(params, false)

    return this.getDb().prepare(`
      SELECT
        l.session_id AS sessionId,
        l.model,
        l.created_at AS createdAt,
        l.input_tokens AS inputTokens,
        l.output_tokens AS outputTokens,
        l.cache_read_tokens AS cacheRead,
        l.cache_creation_tokens AS cacheCreation
      FROM proxy_request_logs l
      ${sql}
        AND l.session_id IS NOT NULL AND l.session_id != ''
        AND l.session_id IN (
          SELECT s.session_id FROM proxy_request_logs s
          ${subSql}
            AND s.session_id IS NOT NULL AND s.session_id != ''
          GROUP BY s.session_id ORDER BY COUNT(*) DESC LIMIT ${SESSION_TOP_N}
        )
      ORDER BY l.session_id, l.created_at
    `).all(...binds, ...subBinds) as SessionRequestToken[]
  }

  // 会话时间戳（用于密度图）
  getSessionTimestamps(sessionIds: string[]): Map<string, number[]> {
    if (sessionIds.length === 0) return new Map()

    const placeholders = sessionIds.map(() => '?').join(',')
    const rows = this.getDb().prepare(`
      SELECT session_id, created_at FROM proxy_request_logs
      WHERE session_id IN (${placeholders})
      ORDER BY session_id, created_at
    `).all(...sessionIds) as { session_id: string; created_at: number }[]

    const result = new Map<string, number[]>()
    for (const row of rows) {
      let arr = result.get(row.session_id)
      if (!arr) {
        arr = []
        result.set(row.session_id, arr)
      }
      arr.push(row.created_at)
    }
    return result
  }

  // 实时 Token 趋势（10 秒桶，最近 1 小时）
  getMinuteLevelTokenTrend(): RealtimeBucket[] {
    const oneHourAgo = Math.floor(Date.now() / 1000) - REALTIME_WINDOW_SEC
    return this.getDb().prepare(`
      SELECT (created_at / 10) * 10 AS bucket,
             COUNT(*) AS requests,
             SUM(input_tokens) AS inputTokens,
             SUM(output_tokens) AS outputTokens,
             SUM(cache_read_tokens) AS cacheRead,
             SUM(cache_creation_tokens) AS cacheCreation
      FROM proxy_request_logs
      WHERE created_at >= ?
      GROUP BY bucket
      ORDER BY bucket
    `).all(oneHourAgo) as RealtimeBucket[]
  }

  getRecentRequestLogsRaw(): { model: string; providerId: string; createdAt: number; inputTokens: number; outputTokens: number; cacheReadTokens: number; cacheCreationTokens: number; latencyMs: number }[] {
    return this.getDb().prepare(`
      SELECT model, provider_id AS providerId, created_at AS createdAt,
             input_tokens AS inputTokens, output_tokens AS outputTokens,
             cache_read_tokens AS cacheReadTokens, cache_creation_tokens AS cacheCreationTokens,
             latency_ms AS latencyMs
      FROM proxy_request_logs
      ORDER BY created_at DESC
      LIMIT 100
    `).all() as any[]
  }
}
