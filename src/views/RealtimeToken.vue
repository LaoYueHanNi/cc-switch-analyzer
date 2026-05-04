<template>
  <div class="realtime-token">
    <!-- 顶部统计卡片 -->
    <div class="realtime-stats">
      <div class="realtime-stat">
        <span class="realtime-stat-label">近 1 小时 Token 数</span>
        <span class="realtime-stat-value">{{ formatNum(totalTokens) }}</span>
      </div>
      <div class="realtime-stat">
        <span class="realtime-stat-label">近 1 小时请求数</span>
        <span class="realtime-stat-value">{{ totalRequests }}</span>
      </div>
      <div class="realtime-stat">
        <span class="realtime-stat-label">上次刷新</span>
        <span class="realtime-stat-value time">{{ lastRefreshTime || '-' }}</span>
      </div>
      <div class="live-badge">
        <span class="live-dot" />
        LIVE
      </div>
    </div>

    <!-- 图表 -->
    <RealtimeAreaChart v-if="buckets.length > 0" :buckets="buckets" />

    <div v-else class="realtime-empty">
      <p>{{ dbStore.hasDatabase ? '暂无实时数据' : '请先选择数据库文件' }}</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { useDatabaseStore } from '@/stores/database'
import { useRealtimePolling } from '@/composables/useRealtimePolling'
import { formatNum } from '@/utils/format'
import RealtimeAreaChart from '@/components/charts/RealtimeAreaChart.vue'

const dbStore = useDatabaseStore()
const { buckets, lastRefreshTime, isPolling, startPolling, stopPolling, refreshNow } = useRealtimePolling()

const totalTokens = computed(() =>
  buckets.value.reduce((sum, b) =>
    sum + (b.inputTokens || 0) + (b.outputTokens || 0) + (b.cacheRead || 0) + (b.cacheCreation || 0), 0
  )
)

const totalRequests = computed(() =>
  buckets.value.reduce((sum, b) => sum + (b.requests || 0), 0)
)

// 进入 Tab 时开始轮询，离开时停止
onMounted(() => {
  if (dbStore.hasDatabase) startPolling()
})

onBeforeUnmount(() => {
  stopPolling()
})

// 数据库加载/切换时立即刷新
watch(() => dbStore.hasDatabase, (val) => {
  if (val) {
    refreshNow()
    startPolling()
  } else {
    stopPolling()
  }
}, { immediate: true })
</script>

<style scoped>
.realtime-token {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.realtime-stats {
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 8px 0 12px;
  flex-shrink: 0;
}

.realtime-stat {
  display: flex;
  flex-direction: column;
}

.realtime-stat-label {
  font-size: 11px;
  color: #999;
}

.realtime-stat-value {
  font-size: 16px;
  font-weight: 600;
  color: #333;
}

.realtime-stat-value.time {
  font-size: 13px;
  font-weight: 400;
  color: #666;
}

.live-badge {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 14px;
  font-weight: 700;
  color: #e74c3c;
  margin-left: auto;
}

.live-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #e74c3c;
  animation: pulse 1.5s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

.realtime-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #999;
}
</style>
