import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'

// 筛选与查询 composable
export function useFilter() {
  const filterStore = useFilterStore()
  const queryStore = useQueryStore()

  // 快捷日期查询
  async function quickDateQuery(days: number): Promise<void> {
    const toDate = new Date()
    const fromDate = new Date()
    fromDate.setDate(fromDate.getDate() - days)
    filterStore.fromDate = fromDate
    filterStore.toDate = toDate
    await queryStore.executeQuery(filterStore.filterParams)
  }

  // 重置筛选
  function resetFilter(): void {
    filterStore.reset()
  }

  return {
    quickDateQuery,
    resetFilter
  }
}
