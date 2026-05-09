<template>
  <div class="pricing-card">
    <div class="pricing-header">
      <div class="pricing-header-left">
        <span class="pricing-name">{{ modelName }}</span>
        <n-button size="tiny" text @click="$emit('edit')">编辑</n-button>
      </div>
      <div v-if="isOverride || activeRule" class="pricing-badges">
        <span v-if="isOverride" class="custom-badge">自定义</span>
        <span v-if="activeRule" class="active-pill">
          <n-icon size="9"><time-outline /></n-icon>
          {{ activeRule.label || '生效中' }}
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
      :input-rate="formatRate(getRate('inputCostPerMillion')) + '/M'"
      :output-rate="formatRate(getRate('outputCostPerMillion')) + '/M'"
      :cache-read-rate="formatRate(getRate('cacheReadCostPerMillion')) + '/M'"
      :cache-creation-rate="formatRate(getRate('cacheCreationCostPerMillion')) + '/M'"
    />

    <!-- 上下文定价档位 -->
    <div v-if="displayTiers.length > 0" class="context-tiers">
      <div v-for="tier in displayTiers" :key="tier.threshold" class="context-tier">
        <span class="tier-threshold">>= {{ Math.round(tier.threshold / 1000) }}K</span>
        <span class="tier-rates">
          <span class="tier-rate" :style="{ color: COLORS.PURPLE }">{{ formatRate(tier.inputCostPerMillion) }}</span>
          <span class="tier-rate" :style="{ color: COLORS.ORANGE }">{{ formatRate(tier.outputCostPerMillion) }}</span>
          <span class="tier-rate" :style="{ color: COLORS.BLUE }">{{ formatRate(tier.cacheReadCostPerMillion) }}</span>
          <span class="tier-rate" :style="{ color: COLORS.DARK_ORANGE }">{{ formatRate(tier.cacheCreationCostPerMillion) }}</span>
        </span>
      </div>
    </div>

    <!-- 时间定价规则 -->
    <div v-if="allTimeRules.length > 0" class="time-rules">
      <div
        v-for="(rule, idx) in allTimeRules"
        :key="idx"
        class="time-rule"
        :class="{ 'time-rule-active': isActive(rule) }"
      >
        <n-icon size="10"><time-outline /></n-icon>
        <div class="time-rule-info">
          <span class="time-rule-label">{{ rule.label || '时段定价' }}</span>
          <span class="time-rule-date">{{ formatDate(rule.startTime) }} ~ {{ formatDate(rule.endTime) }}</span>
        </div>
        <template v-if="!rule.readonly">
          <n-button size="tiny" quaternary class="rule-icon-btn" @click="$emit('editTimeRule', timeRules[idx])">
            <template #icon><n-icon size="11"><create-outline /></n-icon></template>
          </n-button>
          <n-button size="tiny" quaternary class="rule-icon-btn" @click="$emit('deleteTimeRule', timeRules[idx])">
            <template #icon><n-icon size="11"><trash-outline /></n-icon></template>
          </n-button>
        </template>
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
import { formatRate } from '@/utils/format'
import { epochToDateStr } from '@/utils/format'
import { COLORS } from '@/utils/constants'
import PricingGrid from '@/components/common/PricingGrid.vue'
import type { PricingData, TimePricingRule, CloudPricingTimeRule, ContextTier } from '@/types/pricing'

interface DisplayTimeRule {
  label: string
  startTime: number
  endTime: number
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
  contextTiers: ContextTier[]
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
}>(), {
  cloudTimeRules: () => []
})

defineEmits<{
  edit: []
  addTimeRule: []
  editTimeRule: [rule: TimePricingRule]
  deleteTimeRule: [rule: TimePricingRule]
}>()

// 合并用户 + 云端时间规则
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
    readonly: true
  }))
  return [...user, ...cloud]
})

// 当前时间命中时间定价规则
const activeRule = computed(() => {
  const now = Math.floor(Date.now() / 1000)
  return allTimeRules.value.find(r => now >= r.startTime && now <= r.endTime) || null
})

function isActive(rule: DisplayTimeRule): boolean {
  const now = Math.floor(Date.now() / 1000)
  return now >= rule.startTime && now <= rule.endTime
}

// 上下文档位匹配：找 threshold <= contextSize 的最大档位
function resolveTier(tiers: ContextTier[], contextSize: number): ContextTier | null {
  let best: ContextTier | null = null
  for (const tier of tiers) {
    if (tier.threshold <= contextSize && (!best || tier.threshold > best.threshold)) {
      best = tier
    }
  }
  return best
}

// 获取基础单价（不解析上下文档位）
type RateField = 'inputCostPerMillion' | 'outputCostPerMillion' | 'cacheReadCostPerMillion' | 'cacheCreationCostPerMillion'
function getRate(field: RateField): number {
  // 1. 时间规则基础定价
  if (activeRule.value) {
    return activeRule.value[field]
  }
  // 2. 用户覆盖基础定价
  if (props.pricing?.isOverride) {
    return props.pricing[field] || 0
  }
  // 3. 默认定价
  return props.pricing?.[field] || 0
}

// 展示用的上下文档位：命中时间规则时用时间规则的档位，否则用覆盖/默认档位
const displayTiers = computed(() => {
  if (activeRule.value && activeRule.value.contextTiers?.length > 0) {
    return activeRule.value.contextTiers
  }
  return props.contextTiers
})

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
  flex-direction: column;
  gap: 2px;
  margin-bottom: 3px;
}

.pricing-header-left {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
}

.pricing-badges {
  display: flex;
  align-items: center;
  gap: 4px;
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
}

.context-tiers {
  margin-top: 4px;
  display: flex;
  flex-direction: column;
  gap: 1px;
  font-size: 11px;
}

.context-tier {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 4px;
}

.tier-threshold {
  color: var(--text-secondary);
  font-size: 10px;
  flex-shrink: 0;
}

.tier-rates {
  display: flex;
  gap: 4px;
  font-size: 10px;
  justify-content: flex-end;
  flex: 1;
}

.tier-rate {
  white-space: nowrap;
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

.time-rule-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.time-rule-label {
  color: var(--text-muted);
  font-size: 10px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.time-rule-date {
  font-size: 9px;
  color: var(--color-amber);
  opacity: 0.7;
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
