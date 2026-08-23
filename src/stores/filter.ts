import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { FilterParams } from '@/types/database'
import { platformAdapter } from '@/platform'

// 筛选状态管理
export const useFilterStore = defineStore('filter', () => {
  const fromDate = ref<Date | null>(null)
  const toDate = ref<Date | null>(null)
  const activeQuickDays = ref<number | null>(1)
  const providerId = ref('')
  const modelId = ref('')
  // 供应商筛选选项：自 0.7.54 起为数据源粒度（值为数据源 canonical 名，如 "CCS"/"OpenCode"），
  // 不再暴露数据源内部 provider_id（CCS UUID、OpenCode providerID 等）
  const providerOptions = ref<{ label: string; value: string }[]>([])
  const modelOptions = ref<{ label: string; value: string }[]>([])
  const dateRangeMin = ref<number | null>(null)
  const dateRangeMax = ref<number | null>(null)

  // CCS 会话日志同步写入记录过滤（设置页配置，全局查询生效）
  const ccsFilterSessionApps = ref<string[]>([])

  // 从后端设置加载 CCS 会话同步过滤
  async function loadCcsSessionFilter(): Promise<void> {
    try {
      const cfg = await platformAdapter.getCcsSessionFilter()
      ccsFilterSessionApps.value = cfg.enabled ? (cfg.apps ?? []) : []
    } catch {
      // ignore
    }
  }

  // 构建 FilterParams
  const filterParams = computed<FilterParams>(() => ({
    fromDate: fromDate.value,
    toDate: toDate.value,
    providerId: providerId.value || '',
    modelId: modelId.value || '',
    ccsFilterSessionApps: ccsFilterSessionApps.value.length
      ? [...ccsFilterSessionApps.value]
      : undefined,
  }))

  // 设置筛选选项（数据库加载后调用）
  function setOptions(
    providers: { id: string; name: string }[],
    models: string[],
    minDate: number,
    maxDate: number
  ): void {
    providerOptions.value = providers.map(p => ({ label: p.name, value: p.id }))
    modelOptions.value = models.map(m => ({ label: m, value: m }))
    dateRangeMin.value = minDate
    dateRangeMax.value = maxDate
  }

  // 设置日期范围（首次加载，默认当天）
  // 注意：fromDate 也设为 max 是有意为之——配合 activeQuickDays=1 的按钮高亮状态，
  // 首次查询通过 filterParams 走的是 fromDate/toDate，而非 quickDateQuery。
  // 所以 fromDate=toDate=max 实现了"只查今天"的效果。
  function setDateRange(min: number, max: number): void {
    const toDateVal = new Date(max * 1000)
    fromDate.value = new Date(max * 1000)
    toDate.value = toDateVal
  }

  function reset(): void {
    fromDate.value = null
    toDate.value = null
    activeQuickDays.value = null
    providerId.value = ''
    modelId.value = ''
  }

  return {
    fromDate,
    toDate,
    activeQuickDays,
    providerId,
    modelId,
    providerOptions,
    modelOptions,
    dateRangeMin,
    dateRangeMax,
    ccsFilterSessionApps,
    loadCcsSessionFilter,
    filterParams,
    setOptions,
    setDateRange,
    reset
  }
})
