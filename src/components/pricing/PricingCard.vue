<template>
  <div class="pricing-card">
    <div class="pricing-header">
      <div class="pricing-header-left">
        <span class="pricing-name">{{ modelName }}</span>
        <span class="header-actions">
          <button class="text-btn" @click="$emit('manageAliases')">
            别名<span v-if="aliases.length">({{ aliases.length }})</span>
          </button>
          <button class="text-btn" @click="$emit('edit')">编辑</button>
        </span>
      </div>
      <div v-if="isOverride || activeRule" class="pricing-badges">
        <span v-if="isOverride" class="custom-badge">自定义</span>
        <span v-if="activeRule" class="active-pill">
          ⏱ {{ activeRule.label || '生效中' }}
        </span>
      </div>
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
      :input-rate="formatRate(activeRates.inputRate) + '/M'"
      :output-rate="formatRate(activeRates.outputRate) + '/M'"
      :cache-read-rate="formatRate(activeRates.cacheReadRate) + '/M'"
      :cache-creation-rate="formatRate(activeRates.cacheCreationRate) + '/M'"
    />

    <!-- 上下文定价档位 -->
    <div v-if="displayTiers.length > 0" class="context-tiers">
      <div v-for="tier in displayTiers" :key="tier.threshold" class="context-tier">
        <span class="tier-threshold">>= {{ Math.round(tier.threshold / 1000) }}K</span>
        <span class="tier-rates">
          <span class="tier-rate" :style="{ color: 'var(--color-purple)' }">{{ formatRate(tier.inputCostPerMillion) }}</span>
          <span class="tier-rate" :style="{ color: 'var(--color-orange)' }">{{ formatRate(tier.outputCostPerMillion) }}</span>
          <span class="tier-rate" :style="{ color: 'var(--color-blue)' }">{{ formatRate(tier.cacheReadCostPerMillion) }}</span>
          <span class="tier-rate" :style="{ color: 'var(--color-dark-orange)' }">{{ formatRate(tier.cacheCreationCostPerMillion) }}</span>
        </span>
      </div>
    </div>

    <!-- 时间定价规则 + 常驻价（未命中区间时回落） -->
    <div v-if="allTimeRules.length > 0" class="time-rules">
      <div
        v-for="(rule, idx) in allTimeRules"
        :key="idx"
        class="time-rule"
        :class="{ 'time-rule-active': isActive(rule) }"
      >
        <span class="time-icon">⏱</span>
        <div class="time-rule-info">
          <span class="time-rule-label">{{ rule.label || '时段定价' }}</span>
          <span class="time-rule-date">{{ formatDate(rule.startTime) }} ~ {{ formatDate(rule.endTime) }}</span>
          <span v-if="formatDailySlotsSummary(rule.dailySlots)" class="time-rule-date">峰 {{ formatDailySlotsSummary(rule.dailySlots) }}</span>
        </div>
        <template v-if="!rule.readonly">
          <button class="icon-btn" title="编辑" @click="$emit('editTimeRule', timeRules[idx])">✎</button>
          <button class="icon-btn" title="删除" @click="$emit('deleteTimeRule', timeRules[idx])">✕</button>
        </template>
        <template v-else>
          <button class="icon-btn" title="查看" @click="$emit('viewTimeRule', rule)">👁</button>
        </template>
      </div>
      <!-- 仅当时间区间盖住常驻时展示，便于对照回落价；常驻已生效则不再重复 -->
      <div v-if="activeRule" class="time-rule">
        <span class="time-icon">⏱</span>
        <div class="time-rule-info">
          <span class="time-rule-label">{{ baseDisplayRule.label }}</span>
          <span class="time-rule-date">{{ formatDate(baseDisplayRule.startTime) }} ~ {{ formatDate(baseDisplayRule.endTime) }}</span>
          <span v-if="formatDailySlotsSummary(baseDisplayRule.dailySlots)" class="time-rule-date">峰 {{ formatDailySlotsSummary(baseDisplayRule.dailySlots) }}</span>
        </div>
        <button class="icon-btn" title="查看" @click="$emit('viewTimeRule', baseDisplayRule)">👁</button>
      </div>
    </div>

    <button class="add-time-btn" @click="$emit('addTimeRule')">+ 添加时间定价</button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { formatRate } from '@/utils/format'
import { epochToDateStr } from '@/utils/format'
import PricingGrid from '@/components/common/PricingGrid.vue'
import { getActiveRate, formatDailySlotsSummary } from '@/utils/pricing'
import type { PricingData, TimePricingRule, CloudPricingTimeRule, ContextTier, DailySlot } from '@/types/pricing'

interface DisplayTimeRule {
  label: string
  startTime: number
  endTime: number
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  contextTiers: ContextTier[]
  dailySlots?: DailySlot[]
  readonly: boolean
}

const props = withDefaults(defineProps<{
  pricing: PricingData | null
  modelName: string
  computedCost: number
  isOverride: boolean
  timeRules: TimePricingRule[]
  cloudTimeRules: CloudPricingTimeRule[]
  contextTiers: ContextTier[]
  simTokens: { input: number; output: number; cacheRead: number; cacheCreation: number }
  aliases: string[]
}>(), {
  cloudTimeRules: () => [],
  aliases: () => []
})

defineEmits<{
  edit: []
  manageAliases: []
  addTimeRule: []
  editTimeRule: [rule: TimePricingRule]
  deleteTimeRule: [rule: TimePricingRule]
  viewTimeRule: [rule: DisplayTimeRule]
}>()

const allTimeRules = computed<DisplayTimeRule[]>(() => {
  const user: DisplayTimeRule[] = props.timeRules.map(r => ({
    label: r.label,
    startTime: r.startTime,
    endTime: r.endTime,
    inputCostPerMillion: r.inputCostPerMillion,
    outputCostPerMillion: r.outputCostPerMillion,
    cacheReadCostPerMillion: r.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: r.cacheCreationCostPerMillion,
    contextTiers: r.contextTiers || [],
    dailySlots: r.dailySlots || [],
    readonly: false
  }))
  const cloud: DisplayTimeRule[] = props.cloudTimeRules.map(r => ({
    label: r.label,
    startTime: r.startTime,
    endTime: r.endTime,
    inputCostPerMillion: r.inputCostPerMillion,
    outputCostPerMillion: r.outputCostPerMillion,
    cacheReadCostPerMillion: r.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: r.cacheCreationCostPerMillion,
    contextTiers: r.contextTiers || [],
    dailySlots: r.dailySlots || [],
    readonly: true
  }))
  return [...user, ...cloud]
})

const activeRule = computed(() => {
  const now = Math.floor(Date.now() / 1000)
  return allTimeRules.value.find(r => now >= r.startTime && now <= r.endTime) || null
})

/** 常驻价：未命中任何时间规则时回落；展示区间从最晚规则结束后起算 */
const baseDisplayRule = computed<DisplayTimeRule>(() => {
  const maxEnd = allTimeRules.value.reduce((m, r) => Math.max(m, r.endTime), -1)
  return {
    label: '常驻价',
    startTime: maxEnd >= 0 ? maxEnd + 1 : 0,
    endTime: 4102415999,
    inputCostPerMillion: props.pricing?.inputCostPerMillion || 0,
    outputCostPerMillion: props.pricing?.outputCostPerMillion || 0,
    cacheReadCostPerMillion: props.pricing?.cacheReadCostPerMillion || 0,
    cacheCreationCostPerMillion: props.pricing?.cacheCreationCostPerMillion || 0,
    contextTiers: props.contextTiers || [],
    dailySlots: props.pricing?.dailySlots || [],
    readonly: true
  }
})

function isActive(rule: DisplayTimeRule): boolean {
  const now = Math.floor(Date.now() / 1000)
  return now >= rule.startTime && now <= rule.endTime
}

const displayTiers = computed(() => {
  if (activeRule.value && activeRule.value.contextTiers?.length > 0) {
    return activeRule.value.contextTiers
  }
  return props.contextTiers
})

const activeRates = computed(() => {
  const ctx = props.simTokens.input + props.simTokens.cacheRead
  return getActiveRate(props.pricing || undefined, ctx)
})

const inputCostComputed = computed(() =>
  props.simTokens.input * activeRates.value.inputRate / 1_000_000
)
const outputCostComputed = computed(() =>
  props.simTokens.output * activeRates.value.outputRate / 1_000_000
)
const cacheReadCostComputed = computed(() =>
  props.simTokens.cacheRead * activeRates.value.cacheReadRate / 1_000_000
)
const cacheCreationCostComputed = computed(() =>
  props.simTokens.cacheCreation * activeRates.value.cacheCreationRate / 1_000_000
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
  padding: var(--card-padding);
  min-width: 0;
  overflow: hidden;
  transition: border-color var(--transition-speed);
}
.pricing-card:hover { border-color: var(--color-blue); }
.pricing-header { display: flex; flex-direction: column; gap: 2px; margin-bottom: 3px; }
.pricing-header-left { display: flex; align-items: center; justify-content: space-between; gap: 4px; }
.pricing-badges { display: flex; align-items: center; gap: 4px; }
.pricing-name {
  font-size: 12px; font-weight: 600; color: var(--text-primary);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.active-pill {
  display: inline-flex; align-items: center; gap: 2px;
  font-size: 9px; color: var(--color-amber); background: var(--color-amber-bg);
  padding: 0 5px; border-radius: 8px; line-height: 16px; white-space: nowrap; flex-shrink: 0;
}
.pricing-cost { font-size: var(--font-size-cost); font-weight: 700; color: var(--color-cost); margin-bottom: 5px; }
.custom-badge { display: inline-block; font-size: 9px; color: var(--color-green); background: var(--color-teal-bg); padding: 1px 4px; border-radius: 2px; }
.header-actions { display: flex; align-items: center; gap: 0; }
.text-btn {
  font-size: 11px; color: var(--text-tertiary); background: none; border: none;
  cursor: pointer; padding: 0 4px; height: 20px;
}
.text-btn:hover { color: var(--color-blue); }
.context-tiers { margin-top: 4px; display: flex; flex-direction: column; gap: 1px; font-size: 11px; }
.context-tier { display: flex; align-items: center; justify-content: space-between; padding: 0 4px; }
.tier-threshold { color: var(--text-secondary); font-size: 10px; flex-shrink: 0; }
.tier-rates { display: flex; gap: 4px; font-size: 10px; justify-content: flex-end; flex: 1; }
.tier-rate { white-space: nowrap; }
.time-rules { margin-top: 5px; display: flex; flex-direction: column; gap: 2px; }
.time-rule { display: flex; align-items: center; gap: 3px; font-size: 10px; color: var(--color-orange); padding: 1px 0; border-radius: 3px; }
.time-rule-active { background: var(--color-amber-bg); padding: 2px 4px; }
.time-icon { font-size: 9px; flex-shrink: 0; }
.time-rule-info { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.time-rule-label { color: var(--text-muted); font-size: 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.time-rule-date { font-size: 9px; color: var(--color-amber); opacity: 0.7; }
.time-rule-active .time-rule-label { color: var(--color-amber); font-weight: 500; }
.icon-btn {
  width: 18px; height: 18px; border: none; background: none; font-size: 10px;
  color: var(--text-faint); cursor: pointer; border-radius: 2px; padding: 0; flex-shrink: 0;
}
.icon-btn:hover { color: var(--text-tertiary); background: var(--bg-hover); }
.add-time-btn {
  margin-top: 4px; font-size: 10px; border: none; background: none;
  color: var(--color-blue); cursor: pointer; padding: 2px 0;
}
.add-time-btn:hover { opacity: 0.7; }
</style>
