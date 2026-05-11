<template>
  <div class="by-provider">
    <div v-if="queryStore.loading" class="tab-loading">
      <n-spin size="medium" />
      <p>正在查询...</p>
    </div>

    <div v-else-if="providerCards.length === 0" class="tab-empty">
      <p>暂无数据</p>
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
import { computed } from 'vue'
import { NSpin } from 'naive-ui'
import { useQueryStore } from '@/stores/query'
import ProviderCard from '@/components/provider/ProviderCard.vue'

const queryStore = useQueryStore()

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
  })).sort((a, b) => b.totalCost - a.totalCost)
})
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
  color: var(--text-muted);
  gap: 12px;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 10px;
}
</style>
