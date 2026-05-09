import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { join } from 'path'
import { tmpdir } from 'os'
import { existsSync, mkdirSync, rmSync } from 'fs'

// vi.mock 会被提升到文件顶部，不能引用模块级变量
// 所以路径用固定值
vi.mock('../../../electron/main/utils/constants', () => {
  const testDir = join(tmpdir(), 'cc-switch-test-' + process.pid)
  return {
    APP_DB_DIR: testDir,
    APP_DB_PATH: join(testDir, 'test-pricing.db'),
    CLOUD_PRICING_URL: 'http://localhost/test',
    QUERY_VERSION: 1,
    CACHE_WINDOW_DAYS: 30,
    SESSION_TOP_N: 50,
    REALTIME_WINDOW_SEC: 3600,
  }
})

import { AppDbService } from '../../../electron/main/services/app-db'
import type { CloudPricingData } from '../../../electron/main/services/app-db'

const CLOUD_DATA: CloudPricingData = {
  version: 3,
  updatedAt: 1700000000,
  currency: 'RMB',
  models: [
    {
      modelId: 'claude-sonnet-4',
      inputCostPerMillion: 21,
      outputCostPerMillion: 105,
      cacheReadCostPerMillion: 2.1,
      cacheCreationCostPerMillion: 26.25,
      contextTiers: [
        { threshold: 10000, inputCostPerMillion: 31.5, outputCostPerMillion: 157.5, cacheReadCostPerMillion: 3.15, cacheCreationCostPerMillion: 39.375 }
      ],
      timeRules: [
        { label: '夜间', startTime: 1700000000, endTime: 1700086400, inputCostPerMillion: 10.5, outputCostPerMillion: 52.5, cacheReadCostPerMillion: 1.05, cacheCreationCostPerMillion: 13.125, contextTiers: [] }
      ]
    },
    {
      modelId: 'claude-haiku-4',
      inputCostPerMillion: 4.2,
      outputCostPerMillion: 21,
      cacheReadCostPerMillion: 0.42,
      cacheCreationCostPerMillion: 5.25,
      contextTiers: [],
      timeRules: []
    }
  ]
}

let svc: AppDbService
const TEST_DIR = join(tmpdir(), 'cc-switch-test-' + process.pid)

function createService(): AppDbService {
  if (!existsSync(TEST_DIR)) mkdirSync(TEST_DIR, { recursive: true })
  return new AppDbService()
}

function cleanup() {
  try { svc?.close() } catch { /* ok */ }
  try { rmSync(TEST_DIR, { recursive: true, force: true }) } catch { /* ok */ }
}

describe('AppDbService — Schema 初始化', () => {
  afterEach(cleanup)

  it('构造后应存在 6 张表', () => {
    svc = createService()
    const tables = (svc as any).db.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").all().map((r: any) => r.name)
    expect(tables).toContain('settings')
    expect(tables).toContain('session_titles')
    expect(tables).toContain('pricing_overrides')
    expect(tables).toContain('time_pricing_overrides')
    expect(tables).toContain('cloud_pricing_cache')
    expect(tables).toContain('cloud_time_rules')
  })
})

describe('AppDbService — Settings CRUD', () => {
  afterEach(cleanup)

  it('getSetting 查询不存在的 key 返回 null', () => {
    svc = createService()
    expect(svc.getSetting('nonexistent')).toBeNull()
  })

  it('setSetting + getSetting 往返正确', () => {
    svc = createService()
    svc.setSetting('key1', 'value1')
    expect(svc.getSetting('key1')).toBe('value1')
  })

  it('setSetting 覆盖已有值', () => {
    svc = createService()
    svc.setSetting('k', 'v1')
    svc.setSetting('k', 'v2')
    expect(svc.getSetting('k')).toBe('v2')
  })
})

describe('AppDbService — 定价覆盖 CRUD', () => {
  afterEach(cleanup)

  it('saveOverride + getAllOverrides 返回基础覆盖', () => {
    svc = createService()
    svc.saveOverride('model-a', 10, 20, 1, 5)
    const list = svc.getAllOverrides()
    expect(list).toHaveLength(1)
    expect(list[0].modelId).toBe('model-a')
    expect(list[0].inputCostPerMillion).toBe(10)
  })

  it('saveOverrideTier 添加上下文档位', () => {
    svc = createService()
    svc.saveOverride('model-a', 10, 20, 1, 5)
    svc.saveOverrideTier('model-a', 5000, 15, 30, 1.5, 7.5)
    const list = svc.getAllOverrides()
    expect(list[0].contextTiers).toHaveLength(1)
    expect(list[0].contextTiers[0].threshold).toBe(5000)
  })

  it('deleteOverrideTier 仅删除指定档位', () => {
    svc = createService()
    svc.saveOverride('model-a', 10, 20, 1, 5)
    svc.saveOverrideTier('model-a', 5000, 15, 30, 1.5, 7.5)
    svc.deleteOverrideTier('model-a', 5000)
    const list = svc.getAllOverrides()
    expect(list[0].contextTiers).toHaveLength(0)
    // 基础覆盖仍在
    expect(list[0].inputCostPerMillion).toBe(10)
  })

  it('deleteOverride 删除模型所有覆盖（含档位）', () => {
    svc = createService()
    svc.saveOverride('model-a', 10, 20, 1, 5)
    svc.saveOverrideTier('model-a', 5000, 15, 30, 1.5, 7.5)
    svc.deleteOverride('model-a')
    expect(svc.getAllOverrides()).toHaveLength(0)
  })

  it('getAllOverrides 按 modelId 排序', () => {
    svc = createService()
    svc.saveOverride('zzz', 1, 1, 1, 1)
    svc.saveOverride('aaa', 2, 2, 2, 2)
    const list = svc.getAllOverrides()
    expect(list[0].modelId).toBe('aaa')
    expect(list[1].modelId).toBe('zzz')
  })
})

describe('AppDbService — 时间定价 CRUD', () => {
  afterEach(cleanup)

  it('addTimeOverride + getAllTimeOverrides', () => {
    svc = createService()
    const id = svc.addTimeOverride('model-a', 100, 200, 10, 20, 1, 5, 'test')
    expect(id).toBeGreaterThan(0)
    const rules = svc.getAllTimeOverrides()
    expect(rules).toHaveLength(1)
    expect(rules[0].modelId).toBe('model-a')
    expect(rules[0].label).toBe('test')
  })

  it('addTimeOverrideTier 档位嵌套', () => {
    svc = createService()
    svc.addTimeOverride('model-a', 100, 200, 10, 20, 1, 5, 'test')
    svc.addTimeOverrideTier('model-a', 100, 200, 5000, 15, 30, 1.5, 7.5)
    const rules = svc.getAllTimeOverrides()
    expect(rules[0].contextTiers).toHaveLength(1)
    expect(rules[0].contextTiers[0].threshold).toBe(5000)
  })

  it('updateTimeOverride 更新值', () => {
    svc = createService()
    const id = svc.addTimeOverride('model-a', 100, 200, 10, 20, 1, 5, 'old')
    svc.updateTimeOverride(id, 100, 300, 15, 25, 1.5, 6, 'new')
    const rules = svc.getAllTimeOverrides()
    expect(rules[0].label).toBe('new')
    expect(rules[0].endTime).toBe(300)
  })

  it('deleteTimeOverride 删除单行', () => {
    svc = createService()
    const id = svc.addTimeOverride('model-a', 100, 200, 10, 20, 1, 5, 'test')
    svc.deleteTimeOverride(id)
    expect(svc.getAllTimeOverrides()).toHaveLength(0)
  })

  it('deleteTimeOverrideGroup 删除整组', () => {
    svc = createService()
    svc.addTimeOverride('model-a', 100, 200, 10, 20, 1, 5, 'test')
    svc.addTimeOverrideTier('model-a', 100, 200, 5000, 15, 30, 1.5, 7.5)
    svc.deleteTimeOverrideGroup('model-a', 100, 200)
    expect(svc.getAllTimeOverrides()).toHaveLength(0)
  })

  it('同组 (modelId, start, end) 不同 threshold 合并为一个规则', () => {
    svc = createService()
    svc.addTimeOverride('model-a', 100, 200, 10, 20, 1, 5, 'test')
    svc.addTimeOverrideTier('model-a', 100, 200, 5000, 15, 30, 1.5, 7.5)
    svc.addTimeOverrideTier('model-a', 100, 200, 20000, 20, 40, 2, 10)
    const rules = svc.getAllTimeOverrides()
    expect(rules).toHaveLength(1)
    expect(rules[0].contextTiers).toHaveLength(2)
  })
})

describe('AppDbService — 云端定价缓存', () => {
  afterEach(cleanup)

  it('saveCloudPricing + loadCloudPricing 往返', () => {
    svc = createService()
    svc.saveCloudPricing(CLOUD_DATA)
    const { base, tiers, cloudTimeRules } = svc.loadCloudPricing()

    expect(base).toHaveLength(2)
    // loadCloudPricing 按 model_id 排序
    expect(base.map(b => b.modelId).sort()).toEqual(['claude-haiku-4', 'claude-sonnet-4'])

    expect(tiers.has('claude-sonnet-4')).toBe(true)
    expect(tiers.get('claude-sonnet-4')!).toHaveLength(1)
    expect(tiers.get('claude-sonnet-4')![0].threshold).toBe(10000)

    expect(cloudTimeRules.has('claude-sonnet-4')).toBe(true)
    expect(cloudTimeRules.get('claude-sonnet-4')!).toHaveLength(1)
    expect(cloudTimeRules.get('claude-sonnet-4')![0].label).toBe('夜间')
  })

  it('第二次 saveCloudPricing 替换之前的数据', () => {
    svc = createService()
    svc.saveCloudPricing(CLOUD_DATA)
    svc.saveCloudPricing({ version: 4, updatedAt: 1, currency: 'RMB', models: [] })
    const { base } = svc.loadCloudPricing()
    expect(base).toHaveLength(0)
  })

  it('版本号存储在 settings 中', () => {
    svc = createService()
    svc.saveCloudPricing(CLOUD_DATA)
    expect(svc.getSetting('cloud_pricing_version')).toBe('3')
  })
})

describe('AppDbService — 会话标题', () => {
  afterEach(cleanup)

  it('getSessionTitles 传入空数组 → 返回空 Map', () => {
    svc = createService()
    expect(svc.getSessionTitles([]).size).toBe(0)
  })

  it('saveSessionTitle + getSessionTitles 往返正确', () => {
    svc = createService()
    svc.saveSessionTitle('sess-1', 'My Title')
    const map = svc.getSessionTitles(['sess-1'])
    expect(map.get('sess-1')).toBe('My Title')
  })

  it('getSessionTitles 查询不存在的 ID → 返回空 Map', () => {
    svc = createService()
    const map = svc.getSessionTitles(['no-exist'])
    expect(map.size).toBe(0)
  })

  it('saveSessionTitle 覆盖已有标题', () => {
    svc = createService()
    svc.saveSessionTitle('s1', 'old')
    svc.saveSessionTitle('s1', 'new')
    expect(svc.getSessionTitles(['s1']).get('s1')).toBe('new')
  })
})
