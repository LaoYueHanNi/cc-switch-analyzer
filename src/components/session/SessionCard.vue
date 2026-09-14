<template>
  <div class="session-card" :class="{ expanded }" @click="$emit('toggle')">
    <div class="session-row">
      <div class="session-row-main">
        <div class="session-row-title">{{ title ? truncateText(title, 28) : shortId }}</div>
        <div class="session-row-sub">{{ shortId }} · {{ formatRange(startTime, endTime) }}</div>
      </div>
      <span class="session-row-cost">{{ formatCost(totalCost) }}</span>
      <span class="session-row-tok">{{ formatNum(totalTokens) }}</span>
      <span class="session-row-req">{{ requestCount }} 次</span>
    </div>

    <div v-if="expanded" class="session-extra" @click.stop>
      <div class="session-overview">
        <div class="session-meta">{{ requestCount }} 次请求, 持续 {{ formatDuration(durationSec) }}</div>
        <div class="session-meta">最大上下文: {{ formatNum(maxContextWidth) }}</div>
        <div class="session-meta">缓存命中率: {{ formatPercent(cacheHitRate) }}</div>
      </div>
      <div class="session-density">
        <DensityChart
          v-if="timestamps.length > 0"
          :timestamps="timestamps"
          :start-time="startTime"
          :end-time="endTime"
        />
      </div>
      <div class="session-models">
        <ModelBreakdown :items="modelBreakdownWithCosts" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { formatNum, formatCost, formatDuration, formatPercent, epochToDateTimeStr, shortSessionId } from '@/utils/format'
import DensityChart from '@/components/charts/DensityChart.vue'
import ModelBreakdown from '@/components/session/ModelBreakdown.vue'

const props = defineProps<{
  sessionId: string
  title?: string
  project?: string
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
    contextTierCosts?: Array<{ threshold: number; cost: number; tokens: number }>
  }>
  expanded?: boolean
}>()

defineEmits<{
  toggle: []
}>()

const shortId = computed(() => shortSessionId(props.sessionId))

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
    totalCost: m.cost,
    totalTokens: (m.inputTokens || 0) + (m.outputTokens || 0) + (m.cacheReadTokens || 0) + (m.cacheCreationTokens || 0),
    contextTierCosts: m.contextTierCosts || []
  }))
)

function truncateText(text: string, max: number): string {
  if (text.length <= max) return text
  return text.slice(0, max) + '…'
}

function formatRange(start: number, end: number): string {
  return `${epochToDateTimeStr(start)} ~ ${epochToDateTimeStr(end).split(' ')[1]}`
}
</script>

<style scoped>
.session-card {
  padding: 4px 10px 4px;
  border-radius: 10px;
  cursor: pointer;
  transition: background var(--transition-speed);
}
.session-card:hover,
.session-card.expanded {
  background: var(--bg-hover);
}

.session-row {
  display: grid;
  grid-template-columns: 1fr auto auto auto;
  gap: 12px;
  align-items: center;
  min-height: 36px;
}
.session-row-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.session-row-sub {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 1px;
}
.session-row-cost {
  font-weight: 700;
  color: var(--color-cost);
  font-variant-numeric: tabular-nums;
}
.session-row-tok {
  font-weight: 600;
  color: var(--color-green);
  font-variant-numeric: tabular-nums;
}
.session-row-req {
  font-size: 11px;
  color: var(--text-faint);
  min-width: 3.5em;
  text-align: right;
}

.session-extra {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: center;
  padding: 6px 0 10px;
}

.session-overview {
  width: 160px;
  flex-shrink: 0;
}

.session-meta {
  font-size: 11px;
  color: var(--text-muted);
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
