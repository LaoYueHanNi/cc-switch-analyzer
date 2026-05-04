import { platformAdapter } from '@/platform'
import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'

// 筛选与查询 composable
export function useFilter() {
  const filterStore = useFilterStore()
  const queryStore = useQueryStore()

  // 执行查询（组合预计算）
  async function executeQuery(): Promise<void> {
    try {
      const params = filterStore.filterParams
      const result = await platformAdapter.queryPrecompute(params)

      queryStore.setResults({
        summary: result.summary,
        modelBreakdown: result.modelBreakdown,
        providerBreakdown: result.providerBreakdown,
        precomputed: result.precomputed
      })
    } catch (err: any) {
      console.error('查询失败:', err)
    }
  }

  // 快捷日期查询
  async function quickDateQuery(days: number): Promise<void> {
    const toDate = new Date()
    const fromDate = new Date()
    fromDate.setDate(fromDate.getDate() - days)
    filterStore.fromDate = fromDate
    filterStore.toDate = toDate
    await executeQuery()
  }

  // 重置筛选
  function resetFilter(): void {
    filterStore.reset()
  }

  return {
    executeQuery,
    quickDateQuery,
    resetFilter
  }
}
