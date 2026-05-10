import { defineStore } from 'pinia'
import { ref } from 'vue'
import { platformAdapter } from '@/platform'
import type { SummaryData, ModelBreakdown, ProviderBreakdown } from '@/types/database'
import type { FilterParams } from '@/types/database'

// 查询结果缓存 store
export const useQueryStore = defineStore('query', () => {
  const summary = ref<SummaryData | null>(null)
  const totalCost = ref(0)
  const modelBreakdown = ref<ModelBreakdown[]>([])
  const providerBreakdown = ref<ProviderBreakdown[]>([])
  const precomputed = ref<any>(null)
  const queryVersion = ref(0)  // 防竞态
  const unpricedModels = ref<string[]>([])
  const loading = ref(false)
  let lastParamsKey = ''

  function setResults(data: any): void {
    queryVersion.value++
    summary.value = data.summary
    totalCost.value = data.precomputed?.modelCosts
      ? Object.values(data.precomputed.modelCosts as Record<string, number>).reduce((a, b) => a + b, 0)
      : 0
    modelBreakdown.value = data.modelBreakdown || []
    providerBreakdown.value = data.providerBreakdown || []
    precomputed.value = data.precomputed || null
    unpricedModels.value = data.precomputed?.unpricedModels || []
  }

  async function executeQuery(params: FilterParams): Promise<void> {
    const key = JSON.stringify(params)
    if (key === lastParamsKey && summary.value) return
    lastParamsKey = key
    loading.value = true
    try {
      const preResult = await platformAdapter.queryPrecompute(params)
      setResults(preResult)
    } catch {
      try {
        const [result, modelResult, providerResult] = await Promise.all([
          platformAdapter.querySummary(params),
          platformAdapter.queryByModel(params),
          platformAdapter.queryByProvider(params)
        ])
        setResults({
          summary: result,
          modelBreakdown: modelResult,
          providerBreakdown: providerResult,
          precomputed: null
        })
      } catch (err) {
        console.error('查询失败:', err)
      }
    } finally {
      loading.value = false
    }
  }

  function reset(): void {
    summary.value = null
    totalCost.value = 0
    modelBreakdown.value = []
    providerBreakdown.value = []
    precomputed.value = null
    unpricedModels.value = []
    lastParamsKey = ''
  }

  return {
    summary,
    totalCost,
    modelBreakdown,
    providerBreakdown,
    precomputed,
    queryVersion,
    unpricedModels,
    loading,
    setResults,
    executeQuery,
    reset
  }
})
