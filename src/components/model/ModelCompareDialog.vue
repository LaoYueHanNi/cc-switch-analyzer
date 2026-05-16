<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)" title="模型费用对比">
    <n-card style="width: 500px" title="模型费用对比" :bordered="false" size="small">
      <div class="compare-container">
        <p class="compare-desc">
          将 <strong>{{ sourceModel }}</strong> 的 Token 用量按 <strong>目标模型</strong> 的定价计算费用
        </p>

        <!-- 模型选择器 -->
        <n-select
          v-model:value="targetModel"
          :options="modelOptions"
          :filter="fuzzyFilter"
          filterable
          placeholder="选择要对比的模型"
          clearable
          style="margin-bottom: 12px"
        />

        <!-- 缓存回退提示 -->
        <div v-if="comparisonResult?.cacheFallback" class="cache-fallback-hint">
          目标模型不支持缓存计费，缓存命中 token 已计入输入费用
        </div>

        <!-- 对比结果 -->
        <div v-if="comparisonResult" class="compare-result">
          <div class="cost-comparison">
            <div class="cost-item">
              <span class="cost-label-text">{{ sourceModel }} 费用</span>
              <span class="cost-num">{{ formatCost(sourceCost) }}</span>
            </div>
            <div class="cost-item">
              <span class="cost-label-text">{{ targetModel }} 费用</span>
              <span class="cost-num">{{ formatCost(comparisonResult.targetCost) }}</span>
            </div>
            <div class="cost-diff" :class="comparisonResult.ratio >= 1 ? 'increase' : 'decrease'">
              {{ comparisonResult.ratio.toFixed(2) }}x
            </div>
          </div>

          <!-- 四维对比 -->
          <div class="breakdown-compare">
            <!-- 表头 -->
            <div class="compare-header">
              <span class="ch-dot" />
              <span class="ch-label" />
              <span class="ch-col">单价</span>
              <span class="ch-col">费用</span>
              <span class="ch-arrow" />
              <span class="ch-col">单价</span>
              <span class="ch-col">费用</span>
              <span class="ch-diff" />
            </div>
            <div
              v-for="item in comparisonResult.items"
              :key="item.label"
              class="compare-row"
            >
              <span class="compare-dot" :style="{ backgroundColor: item.color }" />
              <span class="compare-label">{{ item.label }}</span>
              <span class="compare-rate">{{ formatRate(item.sourceRate) }}/M</span>
              <span class="compare-cost">{{ formatCost(item.cost) }}</span>
              <span class="compare-arrow">→</span>
              <span class="compare-rate-target">{{ formatRate(item.targetRate) }}/M</span>
              <span class="compare-cost-compare">{{ formatCost(item.targetCost) }}</span>
              <span class="compare-diff-small" :class="item.ratio >= 1 ? 'increase' : 'decrease'">
                {{ item.ratio.toFixed(2) }}x
              </span>
            </div>
          </div>
        </div>

        <div class="compare-actions">
          <n-button size="small" @click="$emit('update:show', false)">关闭</n-button>
        </div>
      </div>
    </n-card>
  </n-modal>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { NModal, NCard, NSelect, NButton } from 'naive-ui'
import { formatCost, formatRate } from '@/utils/format'
import { getActiveRate } from '@/utils/pricing'
import type { ModelBreakdown } from '@/types/database'
import type { PricingData } from '@/types/pricing'
import type { CompareBucket } from '@/types/common'

const props = defineProps<{
  show: boolean
  sourceModel: string
  sourceCost: number
  sourceCostBreakdown: [number, number, number, number]
  modelData: ModelBreakdown | null
  compareBuckets: CompareBucket[]
  allModels: PricingData[]
}>()

defineEmits<{
  'update:show': [value: boolean]
}>()

const targetModel = ref<string | null>(null)

const modelOptions = computed(() =>
  props.allModels
    .filter(m => m.modelId !== props.sourceModel)
    .map(m => ({ label: m.modelId, value: m.modelId }))
)

// 模糊搜索：忽略 - . _ 等分隔符
const normalize = (s: string) => s.replace(/[-_.]/g, '').toLowerCase()
const fuzzyFilter = (pattern: string, option: { label: string }) =>
  normalize(option.label).includes(normalize(pattern))

const comparisonResult = computed(() => {
  if (!targetModel.value || !props.modelData) return null

  const sourcePricing = props.allModels.find(m => m.modelId === props.modelData!.model || m.aliases?.includes(props.modelData!.model))
  const targetPricing = props.allModels.find(m => m.modelId === targetModel.value)
  if (!targetPricing) return null

  const labels = ['输入', '输出', '缓存读取', '缓存写入']
  const colors = ['var(--color-purple)', 'var(--color-orange)', 'var(--color-blue)', 'var(--color-dark-orange)']

  let targetCost = 0
  const targetBreakdown = [0, 0, 0, 0]
  let fallbackUsed = false

  if (props.compareBuckets.length > 0) {
    // 逐桶计算
    for (const bucket of props.compareBuckets) {
      const rates = getActiveRate(targetPricing, bucket.threshold, bucket.representativeEpoch)

      // 缓存回退：目标模型标记为 noCacheSupport 时，cacheRead token 并入 input
      let effectiveInput = bucket.inputTokens
      let effectiveCacheRead = bucket.cacheRead
      if (targetPricing.noCacheSupport && bucket.cacheRead > 0) {
        effectiveInput += bucket.cacheRead
        effectiveCacheRead = 0
        fallbackUsed = true
      }

      const tokens = [effectiveInput, bucket.outputTokens, effectiveCacheRead, bucket.cacheCreation]
      const rateArr = [rates.inputRate, rates.outputRate, rates.cacheReadRate, rates.cacheCreationRate]

      for (let i = 0; i < 4; i++) {
        const c = tokens[i] * rateArr[i] / 1_000_000
        targetBreakdown[i] += c
        targetCost += c
      }
    }
  } else {
    // 降级：用汇总数据计算（但仍应用缓存回退）
    const tokens = {
      input: props.modelData.inputTokens,
      output: props.modelData.outputTokens,
      cacheRead: props.modelData.cacheRead,
      cacheCreation: props.modelData.cacheCreation
    }
    const contextSize = tokens.input + tokens.cacheRead
    const rates = getActiveRate(targetPricing, contextSize)

    let effectiveInput = tokens.input
    let effectiveCacheRead = tokens.cacheRead
    if (targetPricing.noCacheSupport && tokens.cacheRead > 0) {
      effectiveInput += tokens.cacheRead
      effectiveCacheRead = 0
      fallbackUsed = true
    }

    const effectiveTokens = [effectiveInput, tokens.output, effectiveCacheRead, tokens.cacheCreation]
    const rateArr = [rates.inputRate, rates.outputRate, rates.cacheReadRate, rates.cacheCreationRate]
    for (let i = 0; i < 4; i++) {
      const c = effectiveTokens[i] * rateArr[i] / 1_000_000
      targetBreakdown[i] = c
      targetCost += c
    }
  }

  const ratio = props.sourceCost > 0 ? targetCost / props.sourceCost : 0

  // 获取源模型的有效单价（用于展示）
  const sourceRates = getActiveRate(sourcePricing, props.modelData.inputTokens + props.modelData.cacheRead)
  const sourceRateArr = [sourceRates.inputRate, sourceRates.outputRate, sourceRates.cacheReadRate, sourceRates.cacheCreationRate]

  // 获取目标模型的有效单价（用于展示，用汇总的 contextSize）
  const targetDisplayRates = getActiveRate(targetPricing, props.modelData.inputTokens + props.modelData.cacheRead)
  const targetDisplayRateArr = [targetDisplayRates.inputRate, targetDisplayRates.outputRate, targetDisplayRates.cacheReadRate, targetDisplayRates.cacheCreationRate]

  const bd = props.sourceCostBreakdown
  const items = labels.map((label, i) => ({
    label,
    cost: bd[i],
    targetCost: targetBreakdown[i],
    sourceRate: sourceRateArr[i],
    targetRate: targetDisplayRateArr[i],
    color: colors[i],
    ratio: bd[i] > 0 ? targetBreakdown[i] / bd[i] : 0
  }))

  return { targetCost, ratio, items, cacheFallback: fallbackUsed }
})

// 关闭时重置
watch(() => props.show, (val) => {
  if (!val) targetModel.value = null
})
</script>

<style scoped>
.compare-container {
  padding: 12px 0;
}

.compare-desc {
  font-size: 13px;
  color: var(--text-tertiary);
  margin-bottom: 12px;
}

.cache-fallback-hint {
  font-size: 12px;
  color: var(--color-orange);
  background: var(--color-orange-bg, rgba(255, 165, 0, 0.1));
  padding: 6px 10px;
  border-radius: 4px;
  margin-bottom: 12px;
}

.compare-result {
  margin-bottom: 16px;
}

.cost-comparison {
  display: flex;
  align-items: center;
  gap: 16px;
  margin-bottom: 12px;
}

.cost-item {
  display: flex;
  flex-direction: column;
}

.cost-label-text {
  font-size: 12px;
  color: var(--text-muted);
}

.cost-num {
  font-size: var(--font-size-cost);
  font-weight: 600;
  color: var(--color-cost);
}

.cost-diff {
  font-size: 16px;
  font-weight: 700;
}

.cost-diff.increase {
  color: var(--color-cost);
}

.cost-diff.decrease {
  color: var(--color-green);
}

.breakdown-compare {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.compare-header {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  font-size: 10px;
  color: var(--text-muted);
}

.ch-dot { width: 8px; }
.ch-label { width: 52px; }
.ch-col { min-width: 52px; text-align: right; }
.ch-arrow { width: 16px; text-align: center; }
.ch-diff { min-width: 40px; text-align: right; }

.compare-row {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  padding: 4px 8px;
  background: var(--color-blue-bg);
  border-radius: 4px;
}

.compare-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.compare-label {
  width: 52px;
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.compare-rate {
  min-width: 52px;
  color: var(--text-muted);
  font-size: 10px;
  text-align: right;
}

.compare-cost {
  color: var(--color-cost);
  min-width: 52px;
  text-align: right;
}

.compare-arrow {
  width: 16px;
  color: var(--text-muted);
  text-align: center;
  flex-shrink: 0;
}

.compare-rate-target {
  min-width: 52px;
  color: var(--color-blue);
  font-size: 10px;
  text-align: right;
}

.compare-cost-compare {
  color: var(--color-blue);
  min-width: 52px;
  font-weight: 600;
  text-align: right;
}

.compare-diff-small {
  min-width: 40px;
  text-align: right;
  font-weight: 600;
}

.compare-diff-small.increase {
  color: var(--color-cost);
}

.compare-diff-small.decrease {
  color: var(--color-green);
}

.compare-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 12px;
}
</style>
