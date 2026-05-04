<template>
  <div class="by-provider">
    <div v-if="loading" class="tab-loading">
      <n-spin size="medium" />
      <p>正在查询...</p>
    </div>

    <div v-else-if="providerCards.length === 0" class="tab-empty">
      <p>{{ dbStore.hasDatabase ? '暂无数据' : '请先选择数据库文件' }}</p>
    </div>

    <div v-else class="card-grid">
      <ProviderCard
        v-for="card in providerCards"
        :key="card.providerId"
        :name="card.name"
        :total-cost="card.totalCost"
        :request-count="card.requestCount"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { NSpin } from 'naive-ui'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'
import ProviderCard from '@/components/provider/ProviderCard.vue'

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const queryStore = useQueryStore()
const loading = ref(false)

interface ProviderCardData {
  providerId: string
  name: string
  totalCost: number
  requestCount: number
}

const providerCards = computed<ProviderCardData[]>(() => {
  const breakdown = queryStore.providerBreakdown
  const pre = queryStore.precomputed

  return breakdown.map(pb => ({
    providerId: pb.providerId,
    name: pb.providerName,
    totalCost: pre?.providerCosts?.[pb.providerId] || 0,
    requestCount: pb.requests
  }))
})

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
  } finally {
    loading.value = false
  }
}

watch(() => filterStore.filterParams, () => loadData(), { deep: true })
watch(() => dbStore.hasDatabase, (val) => { if (val) loadData() }, { immediate: true })
</script>

<style scoped>
.by-provider {
  min-height: 200px;
}

.tab-loading,
.tab-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 0;
  color: #999;
  gap: 12px;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 10px;
}
</style>
