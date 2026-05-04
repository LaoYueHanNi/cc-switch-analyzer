<template>
  <div class="session-card">
    <!-- 区域一：概览信息 -->
    <div class="session-overview">
      <div class="session-id" :title="sessionId">{{ shortId }}</div>
      <div class="session-cost">{{ formatCost(totalCost) }}</div>
      <div class="session-tokens">{{ formatNum(totalTokens) }} Token</div>
      <div class="session-meta">
        {{ requestCount }} 次请求, 持续 {{ formatDuration(durationSec) }}
      </div>
      <div class="session-time">
        {{ formatRange(startTime, endTime) }}
      </div>
      <div class="session-context">
        上下文: {{ formatNum(maxContextWidth) }}
      </div>
      <div class="session-cache">
        缓存命中率: {{ formatPercent(cacheHitRate) }}
      </div>
    </div>

    <!-- 区域二：密度热力图 -->
    <div class="session-density">
      <DensityChart
        v-if="timestamps.length > 0"
        :timestamps="timestamps"
        :start-time="startTime"
        :end-time="endTime"
      />
    </div>

    <!-- 区域三：模型分解 -->
    <div class="session-models">
      <ModelBreakdown :items="modelBreakdownWithCosts" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { formatNum, formatCost, formatDuration, formatPercent, epochToDateTimeStr } from '@/utils/format'
import DensityChart from '@/components/charts/DensityChart.vue'
import ModelBreakdown from '@/components/session/ModelBreakdown.vue'

const props = defineProps<{
  sessionId: string
  totalCost: number
  totalTokens: number
  requestCount: number
  durationSec: number
  startTime: number
  endTime: number
  maxContextWidth: number
  cacheHitRate: number
  timestamps: number[]
  modelBreakdown: Array<{
    sessionId: string
    model: string
    cost?: number
    inputTokens?: number
    outputTokens?: number
    cacheReadTokens?: number
    cacheCreationTokens?: number
    inputCost?: number
    outputCost?: number
    cacheReadCost?: number
    cacheCreationCost?: number
  }>
}>()

const shortId = computed(() => {
  const parts = props.sessionId.split('-')
  return parts[0] || props.sessionId.slice(0, 8)
})

const modelBreakdownWithCosts = computed(() =>
  props.modelBreakdown.map(m => ({
    model: m.model,
    inputTokens: m.inputTokens || 0,
    outputTokens: m.outputTokens || 0,
    cacheRead: m.cacheReadTokens || 0,
    cacheCreation: m.cacheCreationTokens || 0,
    inputCost: m.inputCost,
    outputCost: m.outputCost,
    cacheReadCost: m.cacheReadCost,
    cacheCreationCost: m.cacheCreationCost,
    totalCost: m.cost
  }))
)

function formatRange(start: number, end: number): string {
  return `${epochToDateTimeStr(start)} ~ ${epochToDateTimeStr(end).split(' ')[1]}`
}
</script>

<style scoped>
.session-card {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  padding: 12px;
  background: #fff;
  border-radius: 8px;
  border: 1px solid #e8e8e8;
  margin-bottom: 10px;
  align-items: center;
}

.session-overview {
  min-width: 140px;
  flex: 0 0 auto;
}

.session-id {
  font-size: 13px;
  font-weight: 600;
  color: #333;
  margin-bottom: 4px;
  cursor: default;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-cost {
  font-size: 18px;
  font-weight: 700;
  color: #e74c3c;
}

.session-tokens {
  font-size: 14px;
  color: #16a085;
  font-weight: 600;
}

.session-meta,
.session-time,
.session-context,
.session-cache {
  font-size: 11px;
  color: #888;
  margin-top: 2px;
}

.session-density {
  min-width: 140px;
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
}

.session-models {
  flex: 1;
  min-width: 200px;
  overflow-x: auto;
}
</style>
