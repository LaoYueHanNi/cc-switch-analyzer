<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)">
    <n-card :title="'计费明细 · ' + modelName" :bordered="false" size="small" style="max-width: 460px; width: 90vw">
      <div class="bd-content">
        <div v-for="rule in rules" :key="rule.key" class="bd-rule">
          <div class="bd-rule-header">
            <span class="bd-rule-dot" :class="{ timed: rule.isTimeRule }" />
            <span class="bd-rule-name">{{ rule.label }}</span>
            <span v-if="rule.dateRange" class="bd-rule-date">{{ rule.dateRange }}</span>
          </div>
          <div v-for="tier in rule.tiers" :key="tier.threshold" class="bd-tier">
            <div class="bd-tier-top">
              <span v-if="tier.threshold > 0" class="bd-tier-badge">≥{{ Math.round(tier.threshold / 1000) }}K</span>
              <span v-else class="bd-tier-badge base">基础</span>
              <span v-if="tier.pct > 0" class="bd-tier-pct">{{ tier.pct }}%</span>
            </div>
            <PricingGrid
              :input-tokens="tier.inputTokens"
              :output-tokens="tier.outputTokens"
              :cache-read-tokens="tier.cacheRead"
              :cache-creation-tokens="tier.cacheCreation"
              :input-cost="tier.inputCost"
              :output-cost="tier.outputCost"
              :cache-read-cost="tier.cacheReadCost"
              :cache-creation-cost="tier.cacheCreationCost"
              :input-rate="fmtR(tier.rates.inputRate)"
              :output-rate="fmtR(tier.rates.outputRate)"
              :cache-read-rate="fmtR(tier.rates.cacheReadRate)"
              :cache-creation-rate="fmtR(tier.rates.cacheCreationRate)"
            />
          </div>
          <div class="bd-subtotal">
            <span>小计</span>
            <span class="bd-subtotal-cost">{{ formatCost(rule.totalCost) }}</span>
          </div>
        </div>
      </div>
    </n-card>
  </n-modal>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { NModal, NCard } from 'naive-ui'
import { formatCost, formatRate, epochToDateStr } from '@/utils/format'
import { resolveBucketPricingRule } from '@/utils/pricing'
import PricingGrid from '@/components/common/PricingGrid.vue'
import type { PricingData, ContextTier } from '@/types/pricing'
import type { CompareBucket } from '@/types/common'

const props = defineProps<{
  show: boolean
  modelName: string
  compareBuckets: CompareBucket[]
  pricing: PricingData | null
}>()

defineEmits<{ 'update:show': [value: boolean] }>()

const fmtR = (v: number) => formatRate(v) + '/M'

interface TierData {
  threshold: number
  inputTokens: number; outputTokens: number; cacheRead: number; cacheCreation: number
  inputCost: number; outputCost: number; cacheReadCost: number; cacheCreationCost: number
  rates: { inputRate: number; outputRate: number; cacheReadRate: number; cacheCreationRate: number }
  pct: number
}

interface RuleData {
  key: string; label: string; dateRange?: string; isTimeRule: boolean; endTime?: number
  tiers: TierData[]; totalCost: number
}

const rules = computed<RuleData[]>(() => {
  if (!props.pricing || !props.compareBuckets.length) return []
  const ruleMap = new Map<string, RuleData>()
  const M = 1_000_000

  for (const bucket of props.compareBuckets) {
    const { rule, rates } = resolveBucketPricingRule(props.pricing, bucket.representativeEpoch, bucket.threshold)
    if (!ruleMap.has(rule.key)) {
      ruleMap.set(rule.key, {
        key: rule.key, label: rule.label, isTimeRule: rule.isTimeRule, endTime: rule.endTime,
        dateRange: rule.startTime ? `${epochToDateStr(rule.startTime)} ~ ${epochToDateStr(rule.endTime!)}` : undefined,
        tiers: [], totalCost: 0
      })
    }
    const rd = ruleMap.get(rule.key)!
    let td = rd.tiers.find(t => t.threshold === bucket.threshold)
    if (!td) {
      td = { threshold: bucket.threshold, inputTokens: 0, outputTokens: 0, cacheRead: 0, cacheCreation: 0, inputCost: 0, outputCost: 0, cacheReadCost: 0, cacheCreationCost: 0, rates, pct: 0 }
      rd.tiers.push(td)
    }
    td.inputTokens += bucket.inputTokens; td.outputTokens += bucket.outputTokens
    td.cacheRead += bucket.cacheRead; td.cacheCreation += bucket.cacheCreation
    td.inputCost += bucket.inputTokens * rates.inputRate / M
    td.outputCost += bucket.outputTokens * rates.outputRate / M
    td.cacheReadCost += bucket.cacheRead * rates.cacheReadRate / M
    td.cacheCreationCost += bucket.cacheCreation * rates.cacheCreationRate / M
  }

  const result = Array.from(ruleMap.values())
  for (const r of result) {
    // 补上定价配置中存在但未被命中的档位
    const allTiers = getAllConfiguredTiers(props.pricing!, r.key)
    for (const ct of allTiers) {
      if (!r.tiers.some(t => t.threshold === ct.threshold)) {
        r.tiers.push({
          threshold: ct.threshold, inputTokens: 0, outputTokens: 0, cacheRead: 0, cacheCreation: 0,
          inputCost: 0, outputCost: 0, cacheReadCost: 0, cacheCreationCost: 0,
          rates: { inputRate: ct.inputCostPerMillion, outputRate: ct.outputCostPerMillion, cacheReadRate: ct.cacheReadCostPerMillion, cacheCreationRate: ct.cacheCreationCostPerMillion },
          pct: 0
        })
      }
    }
    r.tiers.sort((a, b) => a.threshold - b.threshold)
    const ruleTotal = r.tiers.reduce((s, t) => s + t.inputTokens + t.outputTokens + t.cacheRead + t.cacheCreation, 0)
    for (const t of r.tiers) {
      t.pct = ruleTotal > 0 ? Math.round((t.inputTokens + t.outputTokens + t.cacheRead + t.cacheCreation) / ruleTotal * 100) : 0
    }
    r.totalCost = r.tiers.reduce((s, t) => s + t.inputCost + t.outputCost + t.cacheReadCost + t.cacheCreationCost, 0)
  }
  result.sort((a, b) => {
    if (a.isTimeRule !== b.isTimeRule) return a.isTimeRule ? -1 : 1
    return (b.endTime || 0) - (a.endTime || 0)
  })
  return result
})

// 根据 rule key 获取该规则下配置的所有档位定义
function getAllConfiguredTiers(pricing: PricingData, ruleKey: string): ContextTier[] {
  if (ruleKey === 'base') return pricing.contextTiers || []
  if (ruleKey.startsWith('tu-')) {
    const id = Number(ruleKey.slice(3))
    return pricing.timeRules?.find(r => r.id === id)?.contextTiers || []
  }
  if (ruleKey.startsWith('tc-')) {
    const parts = ruleKey.slice(3).split('-')
    const st = Number(parts[0]), et = Number(parts[1])
    return pricing.cloudTimeRules?.find(r => r.startTime === st && r.endTime === et)?.contextTiers || []
  }
  return []
}
</script>

<style scoped>
.bd-content { display: flex; flex-direction: column; gap: 12px; }
.bd-rule { display: flex; flex-direction: column; gap: 4px; }
.bd-rule:not(:last-child) { padding-bottom: 12px; border-bottom: 1px solid var(--border-main); }
.bd-rule-header { display: flex; align-items: center; gap: 4px; margin-bottom: 2px; }
.bd-rule-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--color-green); flex-shrink: 0; }
.bd-rule-dot.timed { background: var(--color-orange); }
.bd-rule-name { font-size: 12px; font-weight: 600; color: var(--text-primary); }
.bd-rule-date { font-size: 10px; color: var(--text-muted); margin-left: auto; }
.bd-tier { display: flex; flex-direction: column; gap: 2px; padding-left: 10px; }
.bd-tier-top { display: flex; align-items: center; gap: 4px; }
.bd-tier-badge { font-size: 10px; color: var(--color-blue); font-weight: 500; }
.bd-tier-badge.base { color: var(--text-muted); }
.bd-tier-pct { font-size: 10px; color: var(--text-muted); }
.bd-subtotal { display: flex; justify-content: space-between; align-items: center; padding: 2px 10px; font-size: 11px; color: var(--text-tertiary); }
.bd-subtotal-cost { color: var(--color-cost); font-weight: 600; }
</style>
