import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'
import { usePricingStore } from '@/stores/pricing'

export function useDatabase() {
  const dbStore = useDatabaseStore()
  const filterStore = useFilterStore()
  const queryStore = useQueryStore()
  const pricingStore = usePricingStore()

  async function autoLoadDatabase(): Promise<boolean> {
    try {
      const sources = await platformAdapter.autoLoadDatabase()
      if (!sources || sources.length === 0) {
        return false
      }

      queryStore.reset()
      await updateFilterOptions()
      await loadPricing()
      dbStore.setSources(sources)

      platformAdapter.fetchCloudPricing().then(() => loadPricing()).catch(() => {})
      return true
    } catch (err: any) {
      console.error('[useDatabase] 自动加载异常:', err)
      return false
    }
  }

  async function selectDatabase(): Promise<boolean> {
    dbStore.setLoading(true)
    try {
      const result = await platformAdapter.selectDatabase()
      if (!result) {
        dbStore.setLoading(false)
        return false
      }

      // selectDatabase 内部已经 load 了，从后端刷新 source 列表
      const sources = await platformAdapter.listDatabases()
      queryStore.reset()
      await updateFilterOptions()
      await loadPricing()
      dbStore.setSources(sources)

      platformAdapter.fetchCloudPricing().then(() => loadPricing()).catch(() => {})
      return true
    } catch (err: any) {
      console.error('[useDatabase] 异常:', err)
      dbStore.setError(err.message || '数据库加载失败')
      return false
    }
  }

  async function addDatabase(filePath: string, dbType?: string): Promise<boolean> {
    try {
      const sources = await platformAdapter.addDatabase(filePath, dbType)
      queryStore.reset()
      await updateFilterOptions()
      await loadPricing()
      dbStore.setSources(sources)

      platformAdapter.fetchCloudPricing().then(() => loadPricing()).catch(() => {})
      return true
    } catch (err: any) {
      console.error('[useDatabase] 添加数据源失败:', err)
      dbStore.setError(err.message || '添加数据源失败')
      return false
    }
  }

  async function removeDatabase(sourceId: string): Promise<void> {
    try {
      const sources = await platformAdapter.removeDatabase(sourceId)
      if (sources.length > 0) {
        await updateFilterOptions()
        queryStore.reset()
      }
      dbStore.setSources(sources)
    } catch (err: any) {
      console.error('[useDatabase] 移除数据源失败:', err)
    }
  }

  async function refreshDatabase(): Promise<void> {
    try {
      const result = await platformAdapter.refreshDatabase()
      if (result.hasNew && result.recordCount != null) {
        const sources = await platformAdapter.listDatabases()
        dbStore.setSources(sources)
        dbStore.refreshVersion++
      }
    } catch (err: any) {
      console.error('刷新失败:', err)
    }
  }

  async function updateFilterOptions(): Promise<void> {
    try {
      const options = await platformAdapter.getFilterOptions()
      if (options.dateRange && options.dateRange.min > 0) {
        filterStore.setOptions(
          options.providers || [],
          options.models || [],
          options.dateRange.min,
          options.dateRange.max
        )
        filterStore.setDateRange(options.dateRange.min, options.dateRange.max)
      }
    } catch (err: any) {
      console.error('[useDatabase] 更新筛选选项失败:', err)
    }
  }

  async function loadPricing(): Promise<void> {
    try {
      const [pricing, families] = await Promise.all([
        platformAdapter.getAllPricing(),
        platformAdapter.getPricingFamilies()
      ])
      pricingStore.pricingData = pricing
      pricingStore.families = families
    } catch (err: any) {
      console.error('定价加载失败:', err)
    }
  }

  async function refreshAfterToggle(): Promise<void> {
    queryStore.reset()
    await updateFilterOptionsPreserveDate()
    dbStore.refreshVersion++
  }

  async function updateFilterOptionsPreserveDate(): Promise<void> {
    try {
      const options = await platformAdapter.getFilterOptions()
      if (options.dateRange && options.dateRange.min > 0) {
        filterStore.setOptions(
          options.providers || [],
          options.models || [],
          options.dateRange.min,
          options.dateRange.max
        )
      }
    } catch (err: any) {
      console.error('[useDatabase] 更新筛选选项失败:', err)
    }
  }

  return {
    autoLoadDatabase,
    selectDatabase,
    addDatabase,
    removeDatabase,
    refreshDatabase,
    refreshAfterToggle,
    loadPricing
  }
}
