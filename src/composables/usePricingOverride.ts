import { ref } from 'vue'
import { useMessage } from 'naive-ui'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { usePricingStore } from '@/stores/pricing'
import { useQueryStore } from '@/stores/query'
import { dailySlotsWindowsOverlap } from '@/utils/pricing'
import type { ContextTier, DailySlot } from '@/types/pricing'

/** 打开定价编辑弹窗所需的最小定价信息（PricingData 天然满足） */
export interface PricingEditTarget {
  modelId: string
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  isOverride: boolean
  contextTiers: ContextTier[]
  dailySlots?: DailySlot[]
}

export interface PricingFormData {
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
  dailySlots: DailySlot[]
}

/**
 * 定价覆盖编辑 composable。
 * 封装 PricingEditDialog 的状态、保存（含上下文档位同步）与定价数据重载，
 * 供定价页与模型卡片（原地新增/编辑定价）复用。
 */
export function usePricingOverride() {
  const pricingStore = usePricingStore()
  const queryStore = useQueryStore()
  const filterStore = useFilterStore()
  const dbStore = useDatabaseStore()
  const message = useMessage()

  const showEditDialog = ref(false)
  const editModelId = ref<string | null>(null)
  const editModelName = ref('')
  const editCurrentPricing = ref({ input: 0, output: 0, cacheRead: 0, cacheCreation: 0 })
  const editShowRestore = ref(false)
  const editContextTiers = ref<ContextTier[]>([])
  const editDailySlots = ref<DailySlot[]>([])

  /** 加载完整定价数据到 store */
  async function loadPricingData(): Promise<void> {
    try {
      const [pricing, families] = await Promise.all([
        platformAdapter.getAllPricing(),
        platformAdapter.getPricingFamilies()
      ])
      pricingStore.pricingData = pricing
      pricingStore.families = families
    } catch (e) { console.error('加载定价数据失败', e) }
  }

  /**
   * 定价变更后让统计查询失效并强制重算。
   * 查询结果带参数级缓存（参数未变直接早退），不主动失效的话
   * 模型页会继续显示旧费用（新设定价仍显示 ¥0.00）。
   */
  async function refreshQueries(): Promise<void> {
    if (!dbStore.hasDatabase) return
    await queryStore.executeQuery(filterStore.filterParams, true)
  }

  /** 打开编辑弹窗；pricing 为空表示该模型尚无定价（原地新增） */
  function openEditDialog(modelId: string, pricing?: PricingEditTarget | null): void {
    editModelId.value = modelId
    editModelName.value = modelId
    editCurrentPricing.value = {
      input: pricing?.inputCostPerMillion || 0,
      output: pricing?.outputCostPerMillion || 0,
      cacheRead: pricing?.cacheReadCostPerMillion || 0,
      cacheCreation: pricing?.cacheCreationCostPerMillion || 0
    }
    editShowRestore.value = pricing?.isOverride || false
    editContextTiers.value = pricing?.contextTiers ? [...pricing.contextTiers.map(t => ({ ...t }))] : []
    editDailySlots.value = pricing?.dailySlots ? [...pricing.dailySlots] : []
    showEditDialog.value = true
  }

  /** 保存定价覆盖（含上下文档位同步） */
  async function onSavePricing(data: PricingFormData, tiers: ContextTier[]): Promise<void> {
    const modelId = editModelId.value
    if (!modelId) return
    if (dailySlotsWindowsOverlap(data.dailySlots || [])) {
      message.warning('模型根峰谷时段窗口存在重叠，请调整')
      return
    }
    for (const tier of tiers) {
      if (dailySlotsWindowsOverlap(tier.dailySlots || [])) {
        message.warning(`档位 >= ${Math.round(tier.threshold / 1000)}K 的峰谷时段窗口存在重叠，请调整`)
        return
      }
    }
    await platformAdapter.setPricingOverride({
      modelId,
      input: data.input,
      output: data.output,
      cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation,
      dailySlots: data.dailySlots || []
    })

    // 同步上下文档位：对比旧档位，删除不再存在的，新增或更新保留的
    const oldTiers = editContextTiers.value || []
    for (const old of oldTiers) {
      if (!tiers.find(t => t.threshold === old.threshold)) {
        await platformAdapter.deleteOverrideContextTier({ modelId, threshold: old.threshold })
      }
    }
    for (const tier of tiers) {
      const old = oldTiers.find(t => t.threshold === tier.threshold)
      if (old) {
        await platformAdapter.deleteOverrideContextTier({ modelId, threshold: old.threshold })
      }
      await platformAdapter.saveOverrideContextTier({
        modelId,
        threshold: tier.threshold,
        input: tier.inputCostPerMillion,
        output: tier.outputCostPerMillion,
        cacheRead: tier.cacheReadCostPerMillion,
        cacheCreation: tier.cacheCreationCostPerMillion,
        dailySlots: tier.dailySlots || []
      })
    }

    await platformAdapter.refreshPricing()
    await loadPricingData()
    await refreshQueries()
  }

  /** 恢复默认定价（删除用户覆盖） */
  async function onRestorePricing(modelId: string): Promise<void> {
    await platformAdapter.removePricingOverride(modelId)
    await platformAdapter.refreshPricing()
    await loadPricingData()
    await refreshQueries()
  }

  return {
    showEditDialog,
    editModelId,
    editModelName,
    editCurrentPricing,
    editShowRestore,
    editContextTiers,
    editDailySlots,
    loadPricingData,
    openEditDialog,
    onSavePricing,
    onRestorePricing
  }
}
