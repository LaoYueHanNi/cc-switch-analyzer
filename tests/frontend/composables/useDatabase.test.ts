import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useDatabase } from '@/composables/useDatabase'
import { platformAdapter } from '@/platform'
import { useFilterStore } from '@/stores/filter'

vi.mock('@/platform', () => ({
  platformAdapter: {
    autoLoadDatabase: vi.fn(),
    getFilterOptions: vi.fn(),
    getAllPricing: vi.fn(),
    getPricingFamilies: vi.fn(),
    fetchCloudPricing: vi.fn()
  }
}))

const sampleSource = {
  id: '1',
  path: 'C:/x/cc-switch.db',
  dbType: 'CCS',
  enabled: true,
  recordCount: 10
} as any

describe('useDatabase 筛选选项填充', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    vi.mocked(platformAdapter.autoLoadDatabase).mockResolvedValue([sampleSource])
    vi.mocked(platformAdapter.getAllPricing).mockResolvedValue({} as any)
    vi.mocked(platformAdapter.getPricingFamilies).mockResolvedValue([] as any)
    vi.mocked(platformAdapter.fetchCloudPricing).mockResolvedValue(undefined as any)
  })

  // 回归：任一已启用但暂无记录的数据源会把 dateRange.min 拉成 0，
  // 此时数据源/模型下拉不能整片变空
  it('存在无记录数据源（dateRange.min = 0）时仍填充数据源与模型选项', async () => {
    vi.mocked(platformAdapter.getFilterOptions).mockResolvedValue({
      providers: [
        { id: 'CCS', name: 'CCS' },
        { id: 'Antigravity', name: 'Antigravity' }
      ],
      models: ['claude-sonnet-4'],
      dateRange: { min: 0, max: 0 }
    })

    const ok = await useDatabase().autoLoadDatabase()
    expect(ok).toBe(true)

    const filterStore = useFilterStore()
    expect(filterStore.providerOptions.map(o => o.value)).toEqual(['CCS', 'Antigravity'])
    expect(filterStore.modelOptions.map(o => o.value)).toEqual(['claude-sonnet-4'])
    // 无效日期范围不改写查询区间，也不写入上下界
    expect(filterStore.fromDate).toBeNull()
    expect(filterStore.toDate).toBeNull()
    expect(filterStore.dateRangeMin).toBeNull()
    expect(filterStore.dateRangeMax).toBeNull()
  })

  it('日期范围有效时填充选项并同时设定默认当天查询区间', async () => {
    vi.mocked(platformAdapter.getFilterOptions).mockResolvedValue({
      providers: [{ id: 'CCS', name: 'CCS' }],
      models: ['claude-sonnet-4'],
      dateRange: { min: 1000, max: 2000 }
    })

    await useDatabase().autoLoadDatabase()

    const filterStore = useFilterStore()
    expect(filterStore.providerOptions.map(o => o.value)).toEqual(['CCS'])
    expect(filterStore.dateRangeMin).toBe(1000)
    expect(filterStore.dateRangeMax).toBe(2000)
    expect(filterStore.fromDate?.getTime()).toBe(2000 * 1000)
    expect(filterStore.toDate?.getTime()).toBe(2000 * 1000)
  })

  it('providers 为空时不覆写已有选项', async () => {
    const filterStore = useFilterStore()
    filterStore.setSourceOptions([{ id: 'CCS', name: 'CCS' }], ['claude-sonnet-4'])
    vi.mocked(platformAdapter.getFilterOptions).mockResolvedValue({
      providers: [],
      models: [],
      dateRange: { min: 0, max: 0 }
    })

    await useDatabase().autoLoadDatabase()

    expect(filterStore.providerOptions.map(o => o.value)).toEqual(['CCS'])
    expect(filterStore.modelOptions.map(o => o.value)).toEqual(['claude-sonnet-4'])
  })
})
