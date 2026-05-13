import { ref } from 'vue'
import type { ContextTier } from '@/types/pricing'

/** ContextTierDialog 的初始数据结构 */
export interface TierEditData {
  threshold: number
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}

/**
 * 上下文档位编辑 composable。
 * 封装 PricingEditDialog 和 TimePricingDialog 中完全相同的档位增删改逻辑。
 */
export function useContextTierEditor() {
  const localTiers = ref<ContextTier[]>([])
  const showTierDialog = ref(false)
  const editingTierIdx = ref<number | null>(null)
  const editingTierData = ref<TierEditData | undefined>(undefined)

  /** 重置编辑状态（通常在弹窗关闭时调用） */
  function resetEditingIdx(): void {
    editingTierIdx.value = null
  }

  /** 添加上下文档位 */
  function onAddTier(): void {
    editingTierIdx.value = null
    editingTierData.value = undefined
    showTierDialog.value = true
  }

  /** 编辑上下文档位 */
  function onEditTier(idx: number): void {
    const tier = localTiers.value[idx]
    editingTierIdx.value = idx
    editingTierData.value = {
      threshold: tier.threshold,
      input: tier.inputCostPerMillion,
      output: tier.outputCostPerMillion,
      cacheRead: tier.cacheReadCostPerMillion,
      cacheCreation: tier.cacheCreationCostPerMillion
    }
    showTierDialog.value = true
  }

  /** 删除上下文档位 */
  function onRemoveTier(idx: number): void {
    localTiers.value.splice(idx, 1)
  }

  /** 确认档位编辑（来自 ContextTierDialog 的回调） */
  function onConfirmTier(data: TierEditData): void {
    const tier: ContextTier = {
      threshold: data.threshold,
      inputCostPerMillion: data.input,
      outputCostPerMillion: data.output,
      cacheReadCostPerMillion: data.cacheRead,
      cacheCreationCostPerMillion: data.cacheCreation
    }
    if (editingTierIdx.value !== null) {
      localTiers.value[editingTierIdx.value] = tier
    } else {
      localTiers.value.push(tier)
    }
    editingTierIdx.value = null
  }

  return {
    localTiers,
    showTierDialog,
    editingTierIdx,
    editingTierData,
    resetEditingIdx,
    onAddTier,
    onEditTier,
    onRemoveTier,
    onConfirmTier
  }
}
