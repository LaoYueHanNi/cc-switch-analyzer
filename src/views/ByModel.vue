<template>
  <div class="by-model">
    <!-- 加载状态 -->
    <div v-if="loading" class="tab-loading">
      <n-spin size="medium" />
      <p>正在查询...</p>
    </div>

    <!-- 空数据 -->
    <div v-else-if="modelCards.length === 0" class="tab-empty">
      <p>{{ dbStore.hasDatabase ? '暂无数据，请调整筛选条件' : '请先选择数据库文件' }}</p>
    </div>

    <!-- 卡片流布局 -->
    <div v-else class="card-grid">
      <ModelCard
        v-for="card in modelCards"
        :key="card.modelData.model"
        :model-data="card.modelData"
        :pricing="card.pricing"
        :cost-breakdown="card.costBreakdown"
        :total-cost="card.totalCost"
        :cache-duration-sec="card.cacheDurationSec"
        :has-time-pricing="card.hasTimePricing"
        :time-rules="card.timeRules"
        :cloud-time-rules="card.cloudTimeRules"
        :context-tier-costs="card.contextTierCosts"
        @compare="onCompare"
        @set-pricing="onSetPricing"
      />
    </div>

    <!-- 模型费用对比弹窗 -->
    <ModelCompareDialog
      v-model:show="showCompare"
      :source-model="compareSourceModel"
      :source-cost="compareSourceCost"
      :source-cost-breakdown="compareCostBreakdown"
      :model-data="compareModelData"
      :all-models="pricingStore.pricingData"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { NSpin } from 'naive-ui'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'
import { usePricingStore } from '@/stores/pricing'
import ModelCard from '@/components/model/ModelCard.vue'
import ModelCompareDialog from '@/components/model/ModelCompareDialog.vue'
import type { ModelBreakdown } from '@/types/database'
import type { PricingData, TimePricingRule, CloudPricingTimeRule } from '@/types/pricing'

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const queryStore = useQueryStore()
const pricingStore = usePricingStore()

const loading = ref(false)

// 模型卡片数据
interface ModelCardData {
  modelData: ModelBreakdown
  pricing: PricingData | null
  costBreakdown: [number, number, number, number]
  totalCost: number
  cacheDurationSec: number
  hasTimePricing: boolean
  timeRules: TimePricingRule[]
  cloudTimeRules: CloudPricingTimeRule[]
  contextTierCosts: Array<{ threshold: number; cost: number }>
}

const modelCards = computed<ModelCardData[]>(() => {
  const breakdown = queryStore.modelBreakdown
  const pre = queryStore.precomputed
  const pricingList = pricingStore.pricingData

  if (!breakdown.length) return []

  // 构建 pricing 查找 Map
  const pricingMap = new Map<string, PricingData>()
  for (const p of pricingList) {
    pricingMap.set(p.modelId, p)
  }

  // 构建 cacheDurations Map
  const cacheDurations = pre?.cacheDurations || {}
  const modelTierCosts: Record<string, Array<{ threshold: number; cost: number }>> = pre?.modelContextTierCosts || {}

  return breakdown.filter(mb =>
    mb.inputTokens > 0 || mb.outputTokens > 0 || mb.cacheRead > 0 || mb.cacheCreation > 0
  ).map(mb => {
    const pricing = pricingMap.get(mb.model) || null
    const costBreakdown: [number, number, number, number] = pre?.modelCostBreakdown?.[mb.model] || [0, 0, 0, 0]
    const totalCost = pre?.modelCosts?.[mb.model] || 0
    const cacheDurationSec = cacheDurations[mb.model] || 0
    const hasTimePricing = pricing?.hasTimePricing || false
    const timeRules = pricing?.timeRules || []
    const cloudTimeRules = pricing?.cloudTimeRules || []
    const contextTierCosts = modelTierCosts[mb.model] || []

    return {
      modelData: mb,
      pricing,
      costBreakdown,
      totalCost,
      cacheDurationSec,
      hasTimePricing,
      timeRules,
      cloudTimeRules,
      contextTierCosts
    }
  })
})

// 模型对比
const showCompare = ref(false)
const compareSourceModel = ref('')
const compareSourceCost = ref(0)
const compareCostBreakdown = ref<[number, number, number, number]>([0, 0, 0, 0])
const compareModelData = ref<ModelBreakdown | null>(null)

function onCompare(modelId: string): void {
  const card = modelCards.value.find(c => c.modelData.model === modelId)
  if (!card) return
  compareSourceModel.value = modelId
  compareSourceCost.value = card.totalCost
  compareCostBreakdown.value = card.costBreakdown
  compareModelData.value = card.modelData
  showCompare.value = true
}

// 跳转到定价 Tab 设置定价
import { useRouter } from 'vue-router'
const router = useRouter()

function onSetPricing(_modelId: string): void {
  router.push({ name: 'pricing' })
}

// 执行查询
async function loadData(): Promise<void> {
  if (!dbStore.hasDatabase) return
  loading.value = true
  try {
    const params = filterStore.filterParams

    try {
      const preResult = await platformAdapter.queryPrecompute(params)
      queryStore.setResults(preResult)
    } catch {
      const [result, modelResult, providerResult] = await Promise.all([
        platformAdapter.querySummary(params),
        platformAdapter.queryByModel(params),
        platformAdapter.queryByProvider(params)
      ])
      queryStore.setResults({
        summary: result,
        modelBreakdown: modelResult,
        providerBreakdown: providerResult,
        precomputed: null
      })
    }
  } catch (err: any) {
    console.error('查询失败:', err)
  } finally {
    loading.value = false
  }
}

// 监视筛选变化自动查询
watch(() => filterStore.filterParams, () => {
  loadData()
}, { deep: true })

// 数据库加载后查询（immediate: 组件挂载时如果 DB 已加载则立即查询）
watch(() => dbStore.hasDatabase, (val) => {
  if (val) loadData()
}, { immediate: true })

// 全局刷新触发
watch(() => dbStore.refreshVersion, () => {
  if (dbStore.hasDatabase) loadData()
})
</script>

<style scoped>
.by-model {
  min-height: 200px;
}

.tab-loading,
.tab-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 0;
  color: var(--text-muted);
  gap: 12px;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 10px;
  padding: 4px 0;
}
</style>
