import { defineStore } from 'pinia'
import { ref } from 'vue'
import { platformAdapter } from '@/platform'
import type { SummaryData, ModelBreakdown, ProviderBreakdown, FilterParams } from '@/types/database'
import type { PrecomputedResult } from '@/types/common'

// 查询结果缓存 store
export const useQueryStore = defineStore('query', () => {
  const summary = ref<SummaryData | null>(null)
  const totalCost = ref(0)
  const modelBreakdown = ref<ModelBreakdown[]>([])
  const providerBreakdown = ref<ProviderBreakdown[]>([])
  const precomputed = ref<PrecomputedResult | null>(null)
  const queryVersion = ref(0)  // 防竞态
  const unpricedModels = ref<string[]>([])
  const loading = ref(false)
  let lastParamsKey = ''

  interface QueryResults {
    summary: SummaryData | null
    modelBreakdown: ModelBreakdown[]
    providerBreakdown: ProviderBreakdown[]
    precomputed: PrecomputedResult | null
  }

  function setResults(data: QueryResults): void {
    queryVersion.value++
    summary.value = data.summary
    totalCost.value = data.precomputed?.modelCosts
      ? Object.values(data.precomputed.modelCosts).reduce((a, b) => a + b, 0)
      : 0
    modelBreakdown.value = data.modelBreakdown || []
    providerBreakdown.value = data.providerBreakdown || []
    precomputed.value = data.precomputed || null
    unpricedModels.value = data.precomputed?.unpricedModels || []
  }

  async function executeQuery(params: FilterParams, force: boolean = false): Promise<void> {
    const key = JSON.stringify(params)
    if (!force && key === lastParamsKey && summary.value) return
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
