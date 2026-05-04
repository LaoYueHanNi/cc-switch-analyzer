import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { SummaryData, ModelBreakdown, ProviderBreakdown } from '@/types/database'

// 查询结果缓存 store
export const useQueryStore = defineStore('query', () => {
  const summary = ref<SummaryData | null>(null)
  const totalCost = ref(0)
  const modelBreakdown = ref<ModelBreakdown[]>([])
  const providerBreakdown = ref<ProviderBreakdown[]>([])
  const precomputed = ref<any>(null)
  const queryVersion = ref(0)  // 防竞态

  function setResults(data: any): void {
    queryVersion.value++
    summary.value = data.summary
    totalCost.value = data.precomputed?.modelCosts
      ? Object.values(data.precomputed.modelCosts as Record<string, number>).reduce((a, b) => a + b, 0)
      : 0
    modelBreakdown.value = data.modelBreakdown || []
    providerBreakdown.value = data.providerBreakdown || []
    precomputed.value = data.precomputed || null
  }

  function reset(): void {
    summary.value = null
    totalCost.value = 0
    modelBreakdown.value = []
    providerBreakdown.value = []
    precomputed.value = null
  }

  return {
    summary,
    totalCost,
    modelBreakdown,
    providerBreakdown,
    precomputed,
    queryVersion,
    setResults,
    reset
  }
})
