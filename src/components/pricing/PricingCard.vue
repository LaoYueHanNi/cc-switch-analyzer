<template>
  <div class="pricing-card">
    <div class="pricing-header">
      <span class="pricing-name">{{ displayName }}</span>
      <n-button size="tiny" @click="$emit('edit')">编辑</n-button>
    </div>

    <div class="pricing-cost">¥{{ formatRate(computedCost) }}</div>

    <PricingGrid
      :input-tokens="simTokens.input"
      :output-tokens="simTokens.output"
      :cache-read-tokens="simTokens.cacheRead"
      :cache-creation-tokens="simTokens.cacheCreation"
      :input-cost="inputCostComputed"
      :output-cost="outputCostComputed"
      :cache-read-cost="cacheReadCostComputed"
      :cache-creation-cost="cacheCreationCostComputed"
      :input-rate="formatRate(getRate('inputCostPerMillion')) + '/M'"
      :output-rate="formatRate(getRate('outputCostPerMillion')) + '/M'"
      :cache-read-rate="formatRate(getRate('cacheReadCostPerMillion')) + '/M'"
      :cache-creation-rate="formatRate(getRate('cacheCreationCostPerMillion')) + '/M'"
    />

    <span v-if="isOverride" class="custom-badge">自定义</span>
    <span v-if="activeRule" class="time-active-badge">
      <n-icon size="10"><time-outline /></n-icon>
      {{ activeRule.label || '时段定价生效中' }}
    </span>

    <!-- 时间定价规则 -->
    <div v-if="timeRules.length > 0" class="time-rules">
      <div v-for="rule in timeRules" :key="rule.id" class="time-rule">
        <n-icon size="12"><time-outline /></n-icon>
        <span class="time-rule-label">{{ rule.label || `${formatDate(rule.startTime)} ~ ${formatDate(rule.endTime)}` }}</span>
        <n-button size="tiny" quaternary @click="$emit('editTimeRule', rule)">编辑</n-button>
        <n-button size="tiny" quaternary @click="$emit('deleteTimeRule', rule.id)">删除</n-button>
      </div>
    </div>

    <n-button size="tiny" quaternary class="add-time-btn" @click="$emit('addTimeRule')">
      添加时间定价
    </n-button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { NButton, NIcon } from 'naive-ui'
import { TimeOutline } from '@vicons/ionicons5'
import { formatRate, formatNum } from '@/utils/format'
import { epochToDateStr } from '@/utils/format'
import PricingGrid from '@/components/common/PricingGrid.vue'
import type { PricingData, TimePricingRule } from '@/types/pricing'

const props = defineProps<{
  pricing: PricingData | null
  displayName: string
  computedCost: number
  isOverride: boolean
  timeRules: TimePricingRule[]
  simTokens: { input: number; output: number; cacheRead: number; cacheCreation: number }
}>()

defineEmits<{
  edit: []
  addTimeRule: []
  editTimeRule: [rule: TimePricingRule]
  deleteTimeRule: [id: number]
}>()

// 当前时间命中时间定价规则
const activeRule = computed(() => {
  const now = Math.floor(Date.now() / 1000)
  return props.timeRules.find(r => now >= r.startTime && now <= r.endTime) || null
})

// 获取有效单价（优先时间定价）
type RateField = 'inputCostPerMillion' | 'outputCostPerMillion' | 'cacheReadCostPerMillion' | 'cacheCreationCostPerMillion'
function getRate(field: RateField): number {
  if (activeRule.value) return activeRule.value[field]
  return props.pricing?.[field] || 0
}

// 根据 simTokens 和有效定价计算每项费用
const inputCostComputed = computed(() =>
  props.simTokens.input * getRate('inputCostPerMillion') / 1_000_000
)
const outputCostComputed = computed(() =>
  props.simTokens.output * getRate('outputCostPerMillion') / 1_000_000
)
const cacheReadCostComputed = computed(() =>
  props.simTokens.cacheRead * getRate('cacheReadCostPerMillion') / 1_000_000
)
const cacheCreationCostComputed = computed(() =>
  props.simTokens.cacheCreation * getRate('cacheCreationCostPerMillion') / 1_000_000
)

function formatDate(ts: number): string {
  return epochToDateStr(ts)
}
</script>

<style scoped>
.pricing-card {
  background: #fff;
  border-radius: 6px;
  border: 1px solid #e8e8e8;
  padding: 10px;
  min-width: 0;
  overflow: hidden;
}

.pricing-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  margin-bottom: 4px;
}

.pricing-name {
  font-size: 13px;
  font-weight: 600;
  color: #333;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pricing-cost {
  font-size: 17px;
  font-weight: 700;
  color: #e74c3c;
  margin-bottom: 6px;
}

.custom-badge {
  display: inline-block;
  font-size: 10px;
  color: #16a085;
  background: #e8f8f5;
  padding: 1px 4px;
  border-radius: 2px;
  margin-top: 4px;
  margin-right: 4px;
}

.time-active-badge {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 10px;
  color: #f39c12;
  background: #fef9e7;
  padding: 1px 4px;
  border-radius: 2px;
  margin-top: 4px;
}

.time-rules {
  margin-top: 6px;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.time-rule {
  display: flex;
  align-items: center;
  gap: 3px;
  font-size: 11px;
  color: #f39c12;
}

.time-rule-label {
  flex: 1;
  color: #666;
}

.add-time-btn {
  margin-top: 6px;
}
</style>
