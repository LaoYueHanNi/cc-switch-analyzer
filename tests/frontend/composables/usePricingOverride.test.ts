import { beforeEach, describe, expect, it, vi } from 'vitest'
import { usePricingOverride, type PricingEditTarget } from '@/composables/usePricingOverride'

const adapter = vi.hoisted(() => ({
  setPricingOverride: vi.fn(),
  removePricingOverride: vi.fn(),
  saveOverrideContextTier: vi.fn(),
  deleteOverrideContextTier: vi.fn(),
  refreshPricing: vi.fn(),
  getAllPricing: vi.fn().mockResolvedValue([]),
  getPricingFamilies: vi.fn().mockResolvedValue([])
}))
vi.mock('@/platform', () => ({ platformAdapter: adapter }))
vi.mock('naive-ui', () => ({ useMessage: () => ({ warning: vi.fn() }) }))
vi.mock('@/stores/pricing', () => ({ usePricingStore: () => ({ pricingData: [], families: [] }) }))
vi.mock('@/stores/database', () => ({ useDatabaseStore: () => ({ hasDatabase: false }) }))
vi.mock('@/stores/filter', () => ({ useFilterStore: () => ({ filterParams: {} }) }))
vi.mock('@/stores/query', () => ({ useQueryStore: () => ({ executeQuery: vi.fn() }) }))

describe('使用别名编辑定价', () => {
  beforeEach(() => vi.clearAllMocks())

  it('从用量模型名打开时，保存、删除档位和恢复使用规范模型 ID', async () => {
    const override = usePricingOverride()
    const pricing: PricingEditTarget = {
      modelId: 'gemini-3.8-flash',
      inputCostPerMillion: 30, outputCostPerMillion: 60,
      cacheReadCostPerMillion: 3, cacheCreationCostPerMillion: 40,
      isOverride: true,
      contextTiers: [{ threshold: 100, inputCostPerMillion: 40, outputCostPerMillion: 80,
        cacheReadCostPerMillion: 4, cacheCreationCostPerMillion: 50 }]
    }
    override.openEditDialog('gemini-3.8-flash-n', pricing)
    expect(override.editModelId.value).toBe('gemini-3.8-flash')
    expect(override.editModelName.value).toBe('gemini-3.8-flash-n')
    await override.onSavePricing({ input: 50, output: 100, cacheRead: 5, cacheCreation: 60, dailySlots: [] }, [])
    expect(adapter.setPricingOverride).toHaveBeenCalledWith(expect.objectContaining({ modelId: 'gemini-3.8-flash', input: 50 }))
    expect(adapter.deleteOverrideContextTier).toHaveBeenCalledWith({ modelId: 'gemini-3.8-flash', threshold: 100 })
    await override.onRestorePricing(override.editModelId.value!)
    expect(adapter.removePricingOverride).toHaveBeenCalledWith('gemini-3.8-flash')
  })

  it('新增未知模型时保留原始 ID', async () => {
    const override = usePricingOverride()
    override.openEditDialog('my-custom-model', null)
    await override.onSavePricing({ input: 1, output: 2, cacheRead: 0, cacheCreation: 0, dailySlots: [] }, [])
    expect(adapter.setPricingOverride).toHaveBeenCalledWith(expect.objectContaining({ modelId: 'my-custom-model' }))
  })
})
