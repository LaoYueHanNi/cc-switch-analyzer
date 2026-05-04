<template>
  <div class="pricing-card">
    <div class="pricing-header">
      <div class="pricing-header-left">
        <span class="pricing-name">{{ displayName }}</span>
        <span v-if="activeRule" class="active-pill">
          <n-icon size="9"><time-outline /></n-icon>
          {{ activeRule.label || '生效中' }}
        </span>
      </div>
      <n-button size="tiny" text @click="$emit('edit')">编辑</n-button>
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

    <!-- 时间定价规则 -->
    <div v-if="timeRules.length > 0" class="time-rules">
      <div
        v-for="rule in timeRules"
        :key="rule.id"
        class="time-rule"
        :class="{ 'time-rule-active': isActive(rule) }"
      >
        <n-icon size="10"><time-outline /></n-icon>
        <span class="time-rule-label">{{ rule.label || `${formatDate(rule.startTime)} ~ ${formatDate(rule.endTime)}` }}</span>
        <n-button size="tiny" quaternary class="rule-icon-btn" @click="$emit('editTimeRule', rule)">
          <template #icon><n-icon size="11"><create-outline /></n-icon></template>
        </n-button>
        <n-button size="tiny" quaternary class="rule-icon-btn" @click="$emit('deleteTimeRule', rule.id)">
          <template #icon><n-icon size="11"><trash-outline /></n-icon></template>
        </n-button>
      </div>
    </div>

    <n-button size="tiny" quaternary class="add-time-btn" @click="$emit('addTimeRule')">
      <template #icon><n-icon size="11"><add-outline /></n-icon></template>
      添加时间定价
    </n-button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { NButton, NIcon } from 'naive-ui'
import { TimeOutline, CreateOutline, TrashOutline, AddOutline } from '@vicons/ionicons5'
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

function isActive(rule: TimePricingRule): boolean {
  const now = Math.floor(Date.now() / 1000)
  return now >= rule.startTime && now <= rule.endTime
}

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
  background: var(--bg-card);
  border-radius: 6px;
  border: 1px solid var(--border-main);
  padding: 8px;
  min-width: 0;
  overflow: hidden;
}

.pricing-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  margin-bottom: 3px;
}

.pricing-header-left {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.pricing-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.active-pill {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 9px;
  color: var(--color-amber);
  background: var(--color-amber-bg);
  padding: 0 5px;
  border-radius: 8px;
  line-height: 16px;
  white-space: nowrap;
  flex-shrink: 0;
}

.pricing-cost {
  font-size: 15px;
  font-weight: 700;
  color: var(--color-cost);
  margin-bottom: 5px;
}

.custom-badge {
  display: inline-block;
  font-size: 9px;
  color: var(--color-green);
  background: var(--color-teal-bg);
  padding: 1px 4px;
  border-radius: 2px;
  margin-top: 4px;
  margin-right: 4px;
}

.time-rules {
  margin-top: 5px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.time-rule {
  display: flex;
  align-items: center;
  gap: 3px;
  font-size: 10px;
  color: var(--color-orange);
  padding: 1px 0;
  border-radius: 3px;
}

.time-rule-active {
  background: var(--color-amber-bg);
  padding: 2px 4px;
}

.time-rule-label {
  flex: 1;
  color: var(--text-muted);
  font-size: 10px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.time-rule-active .time-rule-label {
  color: var(--color-amber);
  font-weight: 500;
}

.rule-icon-btn {
  padding: 0 !important;
  min-width: 18px !important;
  height: 18px !important;
  color: var(--text-faint) !important;
}
.rule-icon-btn:hover {
  color: var(--text-tertiary) !important;
}

.add-time-btn {
  margin-top: 4px;
  font-size: 10px;
}
</style>
