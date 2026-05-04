<template>
  <div class="pricing-calculator">
    <!-- 定价专用工具栏 -->
    <div class="pricing-toolbar">
      <div class="toolbar-row">
        <n-select
          v-model:value="searchModel"
          :options="modelOptions"
          filterable
          clearable
          size="small"
          placeholder="搜索模型..."
          style="width: 240px"
        />
        <n-button size="tiny" @click="searchModel = null">清除</n-button>
      </div>
      <div class="toolbar-row">
        <div class="token-inputs">
          <div class="token-input-group">
            <span class="token-label">输入 Token</span>
            <n-input-number v-model:value="simInput" size="small" :show-button="false" :min="0" :step="1" style="width: 80px" />
            <span class="token-suffix">K</span>
          </div>
          <div class="token-input-group">
            <span class="token-label">缓存读取</span>
            <n-input-number v-model:value="simCacheRead" size="small" :show-button="false" :min="0" :step="1" style="width: 80px" />
            <span class="token-suffix">K</span>
          </div>
          <div class="token-input-group">
            <span class="token-label">输出 Token</span>
            <n-input-number v-model:value="simOutput" size="small" :show-button="false" :min="0" :step="1" style="width: 80px" />
            <span class="token-suffix">K</span>
          </div>
          <div class="token-input-group">
            <span class="token-label">缓存写入</span>
            <n-input-number v-model:value="simCacheCreation" size="small" :show-button="false" :min="0" :step="1" style="width: 80px" />
            <span class="token-suffix">K</span>
          </div>
        </div>
        <div class="rate-input">
          <span class="rate-label">汇率</span>
          <n-input-number v-model:value="exchangeRate" size="small" :show-button="false" :min="0.01" :step="0.1" style="width: 80px" />
        </div>
      </div>
    </div>

    <!-- 已使用模型 -->
    <div class="pricing-section">
      <div class="section-title">已使用模型</div>
      <div class="pricing-grid">
        <PricingCard
          v-for="card in usedCards"
          :key="card.modelId"
          :pricing="card"
          :display-name="card.displayName"
          :computed-cost="card.computedCost"
          :is-override="card?.isOverride || false"
          :time-rules="card?.timeRules || []"
          :sim-tokens="simTokens"
          @edit="onOpenEditDialog(card)"
          @add-time-rule="onAddTimeRule(card.modelId)"
          @edit-time-rule="(rule) => onEditTimeRule(rule)"
          @delete-time-rule="(id) => onDeleteTimeRule(id)"
        />
      </div>
    </div>

    <!-- 未使用模型（折叠） -->
    <div class="pricing-section">
      <div class="section-title collapsible" @click="showUnused = !showUnused">
        <n-icon size="14"><chevron-down-outline v-if="!showUnused" /><chevron-up-outline v-else /></n-icon>
        未使用模型 ({{ unusedCards.length }})
      </div>
      <div v-if="showUnused" class="pricing-grid">
        <PricingCard
          v-for="card in unusedCards"
          :key="card.modelId"
          :pricing="card"
          :display-name="card.displayName"
          :computed-cost="card.computedCost"
          :is-override="card?.isOverride || false"
          :time-rules="card?.timeRules || []"
          :sim-tokens="simTokens"
          @edit="onOpenEditDialog(card)"
          @add-time-rule="onAddTimeRule(card.modelId)"
          @edit-time-rule="(rule) => onEditTimeRule(rule)"
          @delete-time-rule="(id) => onDeleteTimeRule(id)"
        />
      </div>
    </div>

    <!-- 时间定价弹窗 -->
    <TimePricingDialog
      v-model:show="showTimeDialog"
      :is-edit="!!editingTimeRule"
      :initial-data="timeDialogData"
      @confirm="onConfirmTimeRule"
    />

    <!-- 编辑定价弹窗 -->
    <PricingEditDialog
      v-model:show="showEditDialog"
      :model-name="editModelName"
      :current-pricing="editCurrentPricing"
      :show-restore="editShowRestore"
      @save="onSavePricing(editModelId!, $event)"
      @restore="onRestorePricing(editModelId!)"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount } from 'vue'
import { NSelect, NButton, NInputNumber, NIcon } from 'naive-ui'
import { ChevronDownOutline, ChevronUpOutline } from '@vicons/ionicons5'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { usePricingStore } from '@/stores/pricing'
import { formatRate } from '@/utils/format'
import PricingCard from '@/components/pricing/PricingCard.vue'
import PricingEditDialog from '@/components/pricing/PricingEditDialog.vue'
import TimePricingDialog from '@/components/pricing/TimePricingDialog.vue'
import type { PricingData, TimePricingRule } from '@/types/pricing'

const dbStore = useDatabaseStore()
const pricingStore = usePricingStore()

const searchModel = ref<string | null>(null)
const simInput = ref(1)
const simCacheRead = ref(70)
const simOutput = ref(1)
const simCacheCreation = ref(0)
const exchangeRate = ref(7.0)
const showUnused = ref(false)

// 编辑定价弹窗
const showEditDialog = ref(false)
const editModelId = ref<string | null>(null)
const editModelName = ref('')
const editCurrentPricing = ref({ input: 0, output: 0, cacheRead: 0, cacheCreation: 0 })
const editShowRestore = ref(false)

// 时间定价弹窗
const showTimeDialog = ref(false)
const editingTimeRule = ref<TimePricingRule | null>(null)
const currentTimeRuleModelId = ref('')

const timeDialogData = computed(() => {
  if (!editingTimeRule.value) return undefined
  const r = editingTimeRule.value
  return {
    label: r.label,
    startTime: r.startTime,
    endTime: r.endTime,
    input: r.inputCostPerMillion,
    output: r.outputCostPerMillion,
    cacheRead: r.cacheReadCostPerMillion,
    cacheCreation: r.cacheCreationCostPerMillion
  }
})

// 模拟 Token 参数（以千为单位，200ms 防抖）
const debouncedInput = ref(simInput.value)
const debouncedCacheRead = ref(simCacheRead.value)
const debouncedOutput = ref(simOutput.value)
const debouncedCacheCreation = ref(simCacheCreation.value)

let tokenTimer: ReturnType<typeof setTimeout>
function scheduleTokenUpdate(): void {
  clearTimeout(tokenTimer)
  tokenTimer = setTimeout(() => {
    debouncedInput.value = simInput.value
    debouncedCacheRead.value = simCacheRead.value
    debouncedOutput.value = simOutput.value
    debouncedCacheCreation.value = simCacheCreation.value
  }, 200)
}

watch([simInput, simCacheRead, simOutput, simCacheCreation], scheduleTokenUpdate)
onBeforeUnmount(() => clearTimeout(tokenTimer))

const simTokens = computed(() => ({
  input: debouncedInput.value * 1000,
  output: debouncedOutput.value * 1000,
  cacheRead: debouncedCacheRead.value * 1000,
  cacheCreation: debouncedCacheCreation.value * 1000
}))

// 模型选项
const modelOptions = computed(() =>
  pricingStore.pricingData.map(p => ({
    label: p.displayName || p.modelId,
    value: p.modelId
  }))
)

// 卡片数据（含模拟费用计算）
interface CardEntry extends PricingData {
  displayName: string
  computedCost: number
}

const allCards = computed<CardEntry[]>(() => {
  const st = simTokens.value
  const now = Math.floor(Date.now() / 1000)
  return pricingStore.pricingData
    .filter(p => !searchModel.value || p.modelId.includes(searchModel.value) || (p.displayName && p.displayName.includes(searchModel.value)))
    .map(p => {
      const rule = p.timeRules?.find(r => now >= r.startTime && now <= r.endTime)
      const inp = rule ? rule.inputCostPerMillion : p.inputCostPerMillion
      const out = rule ? rule.outputCostPerMillion : p.outputCostPerMillion
      const cr = rule ? rule.cacheReadCostPerMillion : p.cacheReadCostPerMillion
      const cc = rule ? rule.cacheCreationCostPerMillion : p.cacheCreationCostPerMillion
      const cost = inp * st.input / 1_000_000 + out * st.output / 1_000_000
        + cr * st.cacheRead / 1_000_000 + cc * st.cacheCreation / 1_000_000
      return {
        ...p,
        displayName: p.displayName || p.modelId,
        computedCost: cost
      }
    })
    .sort((a, b) => b.computedCost - a.computedCost)
})

const usedCards = computed(() => allCards.value.filter(c => c.isUsed))
const unusedCards = computed(() => allCards.value.filter(c => !c.isUsed))

// 打开编辑弹窗
function onOpenEditDialog(card: CardEntry): void {
  editModelId.value = card.modelId
  editModelName.value = card.displayName
  editCurrentPricing.value = {
    input: card.inputCostPerMillion || 0,
    output: card.outputCostPerMillion || 0,
    cacheRead: card.cacheReadCostPerMillion || 0,
    cacheCreation: card.cacheCreationCostPerMillion || 0
  }
  editShowRestore.value = card.isOverride || false
  showEditDialog.value = true
}

// 加载汇率
async function loadExchangeRate(): Promise<void> {
  try {
    const rate = await platformAdapter.getExchangeRate()
    exchangeRate.value = rate
    pricingStore.exchangeRate = rate
  } catch { /* ignore */ }
}

// 汇率防抖保存（500ms）
let rateTimer: ReturnType<typeof setTimeout>
watch(exchangeRate, (val) => {
  clearTimeout(rateTimer)
  rateTimer = setTimeout(async () => {
    await platformAdapter.setExchangeRate(val)
    pricingStore.exchangeRate = val
    await platformAdapter.refreshPricing()
    await loadPricingData()
  }, 500)
})

// 加载完整定价数据
async function loadPricingData(): Promise<void> {
  try {
    const pricing = await platformAdapter.getAllPricing()
    pricingStore.pricingData = pricing
  } catch { /* ignore */ }
}

// 保存定价覆盖
async function onSavePricing(modelId: string, data: { input: number; output: number; cacheRead: number; cacheCreation: number }): Promise<void> {
  await platformAdapter.setPricingOverride({
    modelId,
    input: data.input,
    output: data.output,
    cacheRead: data.cacheRead,
    cacheCreation: data.cacheCreation
  })
  await platformAdapter.refreshPricing()
  await loadPricingData()
}

// 恢复默认定价
async function onRestorePricing(modelId: string): Promise<void> {
  await platformAdapter.removePricingOverride(modelId)
  await platformAdapter.refreshPricing()
  await loadPricingData()
}

// 时间定价 CRUD
function onAddTimeRule(modelId: string): void {
  currentTimeRuleModelId.value = modelId
  editingTimeRule.value = null
  showTimeDialog.value = true
}

function onEditTimeRule(rule: TimePricingRule): void {
  currentTimeRuleModelId.value = rule.modelId
  editingTimeRule.value = rule
  showTimeDialog.value = true
}

async function onConfirmTimeRule(data: { label: string; startTime: number; endTime: number; input: number; output: number; cacheRead: number; cacheCreation: number }): Promise<void> {
  if (editingTimeRule.value) {
    await platformAdapter.updateTimePricingRule({
      id: editingTimeRule.value.id,
      startTime: data.startTime,
      endTime: data.endTime,
      input: data.input,
      output: data.output,
      cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation,
      label: data.label
    })
  } else {
    await platformAdapter.addTimePricingRule({
      modelId: currentTimeRuleModelId.value,
      startTime: data.startTime,
      endTime: data.endTime,
      input: data.input,
      output: data.output,
      cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation,
      label: data.label
    })
  }
  await platformAdapter.refreshPricing()
  await loadPricingData()
  editingTimeRule.value = null
}

async function onDeleteTimeRule(id: number): Promise<void> {
  await platformAdapter.deleteTimePricingRule(id)
  await platformAdapter.refreshPricing()
  await loadPricingData()
}

// 初始化
watch(() => dbStore.hasDatabase, async (val) => {
  if (val) {
    await loadExchangeRate()
    await loadPricingData()
  }
}, { immediate: true })
</script>

<style scoped>
.pricing-calculator {
  display: flex;
  flex-direction: column;
  min-height: 200px;
}

.pricing-toolbar {
  padding: 8px 0 12px;
  border-bottom: 1px solid var(--border-light);
  margin-bottom: 12px;
}

.toolbar-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}

.token-inputs {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.token-input-group {
  display: flex;
  align-items: center;
  gap: 2px;
}

.token-label {
  font-size: 11px;
  color: var(--text-tertiary);
  white-space: nowrap;
}

.token-suffix {
  font-size: 11px;
  color: var(--text-muted);
}

.rate-input {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-left: 12px;
}

.rate-label {
  font-size: 12px;
  color: var(--text-tertiary);
}

.pricing-section {
  margin-bottom: 16px;
}

.section-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 8px;
  display: flex;
  align-items: center;
  gap: 4px;
}

.section-title.collapsible {
  cursor: pointer;
}

.pricing-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 8px;
}
</style>
