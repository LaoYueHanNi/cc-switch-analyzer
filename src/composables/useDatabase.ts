import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'
import { usePricingStore } from '@/stores/pricing'

// 数据库操作 composable
export function useDatabase() {
  const dbStore = useDatabaseStore()
  const filterStore = useFilterStore()
  const queryStore = useQueryStore()
  const pricingStore = usePricingStore()

  // 自动加载默认数据库（启动时调用）
  async function autoLoadDatabase(): Promise<boolean> {
    try {
      console.log('[useDatabase] 尝试自动加载默认数据库...')
      const result = await platformAdapter.autoLoadDatabase()
      if (!result) {
        console.log('[useDatabase] 默认数据库不存在，等待手动选择')
        return false
      }

      dbStore.setLoaded(result.path, result.recordCount)

      if (result.dateRange) {
        filterStore.setOptions(
          result.providers || [],
          result.models || [],
          result.dateRange.min,
          result.dateRange.max
        )
        filterStore.setDateRange(result.dateRange.min, result.dateRange.max)
      }

      await loadPricing()
      queryStore.reset()

      // 异步拉取云端定价（不阻塞启动）
      platformAdapter.fetchCloudPricing().catch(() => {})

      return true
    } catch (err: any) {
      console.error('[useDatabase] 自动加载异常:', err)
      return false
    }
  }

  // 选择数据库文件
  async function selectDatabase(): Promise<boolean> {
    dbStore.setLoading(true)
    try {
      console.log('[useDatabase] 打开文件对话框...')
      const result = await platformAdapter.selectDatabase()

      if (!result) {
        console.log('[useDatabase] 用户取消选择')
        dbStore.setLoading(false)
        return false
      }

      console.log('[useDatabase] 选择的文件:', result.path)

      dbStore.setLoaded(result.path, result.recordCount)
      console.log('[useDatabase] setLoaded 完成, isLoaded=', dbStore.isLoaded)

      if (result.dateRange) {
        filterStore.setOptions(
          result.providers || [],
          result.models || [],
          result.dateRange.min,
          result.dateRange.max
        )
        filterStore.setDateRange(result.dateRange.min, result.dateRange.max)
        console.log('[useDatabase] 筛选选项已设置')
      }

      await loadPricing()
      queryStore.reset()

      // 异步拉取云端定价（不阻塞启动）
      platformAdapter.fetchCloudPricing().catch(() => {})

      return true
    } catch (err: any) {
      console.error('[useDatabase] 异常:', err)
      dbStore.setError(err.message || '数据库加载失败')
      return false
    }
  }

  // 刷新数据库
  async function refreshDatabase(): Promise<void> {
    try {
      const result = await platformAdapter.refreshDatabase()
      if (result.hasNew && result.recordCount != null) {
        dbStore.recordCount = result.recordCount
        dbStore.refreshVersion++
      }
    } catch (err: any) {
      console.error('刷新失败:', err)
    }
  }

  // 加载定价数据
  async function loadPricing(): Promise<void> {
    try {
      const pricing = await platformAdapter.getAllPricing()
      pricingStore.pricingData = pricing
      console.log('[useDatabase] 定价加载完成, 数量:', pricing.length)
    } catch (err: any) {
      console.error('定价加载失败:', err)
    }
  }

  return {
    autoLoadDatabase,
    selectDatabase,
    refreshDatabase,
    loadPricing
  }
}
