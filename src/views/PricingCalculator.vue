<template>
  <div class="pricing-calculator">
    <!-- 定价专用工具栏 -->
    <div class="pricing-toolbar">
      <div class="toolbar-row">
        <div class="filter-group">
          <span class="filter-label">模型</span>
          <n-select
            v-model:value="searchModel"
            :options="modelOptions"
            filterable
            clearable
            size="tiny"
            placeholder="全部"
            style="width: 180px"
            teleport-disabled
          />
        </div>
        <div class="filter-group">
          <span class="filter-label">输入</span>
          <n-input-number v-model:value="simInput" size="tiny" :show-button="false" :min="0" :step="1" style="width: 52px" />
          <span class="filter-label">K</span>
        </div>
        <div class="filter-group">
          <span class="filter-label">缓存读</span>
          <n-input-number v-model:value="simCacheRead" size="tiny" :show-button="false" :min="0" :step="1" style="width: 52px" />
          <span class="filter-label">K</span>
        </div>
        <div class="filter-group">
          <span class="filter-label">输出</span>
          <n-input-number v-model:value="simOutput" size="tiny" :show-button="false" :min="0" :step="1" style="width: 52px" />
          <span class="filter-label">K</span>
        </div>
        <div class="filter-group">
          <span class="filter-label">缓存写</span>
          <n-input-number v-model:value="simCacheCreation" size="tiny" :show-button="false" :min="0" :step="1" style="width: 52px" />
          <span class="filter-label">K</span>
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
          :model-name="card.modelId"
          :computed-cost="card.computedCost"
          :is-override="card?.isOverride || false"
          :time-rules="card?.timeRules || []"
          :cloud-time-rules="card?.cloudTimeRules || []"
          :context-tiers="card?.contextTiers || []"
          :sim-tokens="simTokens"
          :aliases="card.aliases || []"
          @edit="onOpenEditDialog(card)"
          @add-time-rule="onAddTimeRule(card.modelId)"
          @edit-time-rule="(rule) => onEditTimeRule(rule)"
          @delete-time-rule="(rule) => onDeleteTimeRule(rule)"
          @view-time-rule="(rule) => onViewTimeRule(rule)"
          @manage-aliases="onManageAliases(card)"
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
          :model-name="card.modelId"
          :computed-cost="card.computedCost"
          :is-override="card?.isOverride || false"
          :time-rules="card?.timeRules || []"
          :cloud-time-rules="card?.cloudTimeRules || []"
          :context-tiers="card?.contextTiers || []"
          :sim-tokens="simTokens"
          :aliases="card.aliases || []"
          @edit="onOpenEditDialog(card)"
          @add-time-rule="onAddTimeRule(card.modelId)"
          @edit-time-rule="(rule) => onEditTimeRule(rule)"
          @delete-time-rule="(rule) => onDeleteTimeRule(rule)"
          @view-time-rule="(rule) => onViewTimeRule(rule)"
          @manage-aliases="onManageAliases(card)"
        />
      </div>
    </div>

    <!-- 别名管理弹窗 -->
    <AliasDialog
      v-model:show="showAliasDialog"
      :model-id="aliasModelId"
      :model-name="aliasModelName"
      :current-aliases="aliasCurrentAliases"
      :user-aliases="aliasUserAliases"
      @changed="onAliasChanged"
    />

    <!-- 时间定价弹窗 -->
    <TimePricingDialog
      v-model:show="showTimeDialog"
      :is-edit="!!editingTimeRule"
      :readonly="isTimeDialogReadonly"
      :initial-data="timeDialogData"
      :context-tiers="editingTimeRule?.contextTiers || viewingCloudRule?.contextTiers || []"
      @confirm="onConfirmTimeRule"
    />

    <!-- 编辑定价弹窗 -->
    <PricingEditDialog
      v-model:show="showEditDialog"
      :model-name="editModelName"
      :current-pricing="editCurrentPricing"
      :show-restore="editShowRestore"
      :context-tiers="editContextTiers"
      @save="onSavePricing"
      @restore="onRestorePricing(editModelId!)"
    />

  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount } from 'vue'
import { NSelect, NInputNumber, NIcon, useMessage } from 'naive-ui'
import { ChevronDownOutline, ChevronUpOutline } from '@vicons/ionicons5'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { usePricingStore } from '@/stores/pricing'
import PricingCard from '@/components/pricing/PricingCard.vue'
import PricingEditDialog from '@/components/pricing/PricingEditDialog.vue'
import TimePricingDialog from '@/components/pricing/TimePricingDialog.vue'
import AliasDialog from '@/components/pricing/AliasDialog.vue'
import { getActiveRate } from '@/utils/pricing'
import type { PricingData, TimePricingRule, CloudPricingTimeRule, ContextTier } from '@/types/pricing'
import { epochToDateStr } from '@/utils/format'

const dbStore = useDatabaseStore()
const pricingStore = usePricingStore()
const message = useMessage()

const searchModel = ref<string | null>(null)
const simInput = ref(1)
const simCacheRead = ref(70)
const simOutput = ref(1)
const simCacheCreation = ref(0)
const showUnused = ref(false)

// 编辑定价弹窗
const showEditDialog = ref(false)
const editModelId = ref<string | null>(null)
const editModelName = ref('')
const editCurrentPricing = ref({ input: 0, output: 0, cacheRead: 0, cacheCreation: 0 })
const editShowRestore = ref(false)
const editContextTiers = ref<ContextTier[]>([])

// 时间定价弹窗
const showTimeDialog = ref(false)
const editingTimeRule = ref<TimePricingRule | null>(null)
const viewingCloudRule = ref<CloudPricingTimeRule | null>(null)
const currentTimeRuleModelId = ref('')

const isTimeDialogReadonly = computed(() => !!viewingCloudRule.value)

// 别名管理弹窗
const showAliasDialog = ref(false)
const aliasModelId = ref('')
const aliasModelName = ref('')
const aliasCurrentAliases = ref<string[]>([])
const aliasUserAliases = ref<string[]>([])

function onManageAliases(card: PricingData): void {
  aliasModelId.value = card.modelId
  aliasModelName.value = card.modelId
  aliasCurrentAliases.value = card.aliases || []
  aliasUserAliases.value = card.userAliases || []
  showAliasDialog.value = true
}

async function onAliasChanged(): Promise<void> {
  await platformAdapter.refreshPricing()
  loadPricingData()
}

const timeDialogData = computed(() => {
  if (editingTimeRule.value) {
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
  }
  if (viewingCloudRule.value) {
    const r = viewingCloudRule.value
    return {
      label: r.label,
      startTime: r.startTime,
      endTime: r.endTime,
      input: r.inputCostPerMillion,
      output: r.outputCostPerMillion,
      cacheRead: r.cacheReadCostPerMillion,
      cacheCreation: r.cacheCreationCostPerMillion
    }
  }
  return undefined
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
    label: p.modelId,
    value: p.modelId
  }))
)

// 卡片数据（含模拟费用计算）
interface CardEntry extends PricingData {
  modelName: string
  computedCost: number
}

const allCards = computed<CardEntry[]>(() => {
  const st = simTokens.value
  const contextSize = st.input + st.cacheRead
  return pricingStore.pricingData
    .filter(p => !searchModel.value || p.modelId.includes(searchModel.value) || (p.aliases && p.aliases.some((a: string) => a.includes(searchModel.value))))
    .map(p => {
      const rates = getActiveRate(p, contextSize)
      const cost = rates.inputRate * st.input / 1_000_000
        + rates.outputRate * st.output / 1_000_000
        + rates.cacheReadRate * st.cacheRead / 1_000_000
        + rates.cacheCreationRate * st.cacheCreation / 1_000_000
      return {
        ...p,
        modelName: p.modelId,
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
  editModelName.value = card.modelId
  editCurrentPricing.value = {
    input: card.inputCostPerMillion || 0,
    output: card.outputCostPerMillion || 0,
    cacheRead: card.cacheReadCostPerMillion || 0,
    cacheCreation: card.cacheCreationCostPerMillion || 0
  }
  editShowRestore.value = card.isOverride || false
  editContextTiers.value = card.contextTiers ? [...card.contextTiers.map(t => ({ ...t }))] : []
  showEditDialog.value = true
}

// 加载完整定价数据
async function loadPricingData(): Promise<void> {
  try {
    const pricing = await platformAdapter.getAllPricing()
    pricingStore.pricingData = pricing
  } catch (e) { console.error('加载定价数据失败', e) }
}

// 保存定价覆盖
async function onSavePricing(data: { input: number; output: number; cacheRead: number; cacheCreation: number }, tiers: ContextTier[]): Promise<void> {
  const modelId = editModelId.value!
  await platformAdapter.setPricingOverride({
    modelId,
    input: data.input,
    output: data.output,
    cacheRead: data.cacheRead,
    cacheCreation: data.cacheCreation
  })

  // 同步上下文档位：对比旧档位，删除不再存在的，新增或更新保留的
  const oldTiers = editContextTiers.value || []
  for (const old of oldTiers) {
    if (!tiers.find(t => t.threshold === old.threshold)) {
      await platformAdapter.deleteOverrideContextTier({ modelId, threshold: old.threshold })
    }
  }
  for (const tier of tiers) {
    const old = oldTiers.find(t => t.threshold === tier.threshold)
    if (old) {
      await platformAdapter.deleteOverrideContextTier({ modelId, threshold: old.threshold })
    }
    await platformAdapter.saveOverrideContextTier({
      modelId,
      threshold: tier.threshold,
      input: tier.inputCostPerMillion,
      output: tier.outputCostPerMillion,
      cacheRead: tier.cacheReadCostPerMillion,
      cacheCreation: tier.cacheCreationCostPerMillion
    })
  }

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
  viewingCloudRule.value = null
  showTimeDialog.value = true
}

function onViewTimeRule(rule: { label: string; startTime: number; endTime: number; inputCostPerMillion: number; outputCostPerMillion: number; cacheReadCostPerMillion: number; cacheCreationCostPerMillion: number; contextTiers: ContextTier[] }): void {
  editingTimeRule.value = null
  viewingCloudRule.value = {
    label: rule.label,
    startTime: rule.startTime,
    endTime: rule.endTime,
    inputCostPerMillion: rule.inputCostPerMillion,
    outputCostPerMillion: rule.outputCostPerMillion,
    cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
    contextTiers: rule.contextTiers
  }
  showTimeDialog.value = true
}

async function onConfirmTimeRule(data: { label: string; startTime: number; endTime: number; input: number; output: number; cacheRead: number; cacheCreation: number }, tiers: ContextTier[]): Promise<void> {
  if (isTimeDialogReadonly.value) {
    showTimeDialog.value = false
    viewingCloudRule.value = null
    return
  }

  const modelId = currentTimeRuleModelId.value

  // 检查时间冲突：云端规则 + 其他用户规则
  const cloudRules = pricingStore.pricingData.find(p => p.modelId === modelId)?.cloudTimeRules || []
  const userRules = pricingStore.pricingData.find(p => p.modelId === modelId)?.timeRules || []

  const cloudConflict = cloudRules.find(r => data.startTime < r.endTime && data.endTime > r.startTime)
  if (cloudConflict) {
    message.warning(`时间区间与云端规则「${cloudConflict.label || '时段定价'}」（${epochToDateStr(cloudConflict.startTime)} ~ ${epochToDateStr(cloudConflict.endTime)}）冲突，请调整时间范围`)
    return
  }

  // 编辑时排除自身，新增时检查全部
  const otherUserRules = editingTimeRule.value
    ? userRules.filter(r => r.id !== editingTimeRule.value.id)
    : userRules
  const userConflict = otherUserRules.find(r => data.startTime < r.endTime && data.endTime > r.startTime)
  if (userConflict) {
    message.warning(`时间区间与已有规则「${userConflict.label || '时段定价'}」（${epochToDateStr(userConflict.startTime)} ~ ${epochToDateStr(userConflict.endTime)}）冲突，请调整时间范围`)
    return
  }

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

    // 同步上下文档位：对比旧档位，删除不再存在的，新增或更新保留的
    const oldTiers = editingTimeRule.value.contextTiers || []
    for (const old of oldTiers) {
      if (!tiers.find(t => t.threshold === old.threshold)) {
        if (old.id) await platformAdapter.deleteTimeRuleContextTier(old.id)
      }
    }
    for (const tier of tiers) {
      const old = oldTiers.find(t => t.threshold === tier.threshold)
      if (old?.id) {
        await platformAdapter.updateTimeRuleContextTier({
          id: old.id,
          input: tier.inputCostPerMillion,
          output: tier.outputCostPerMillion,
          cacheRead: tier.cacheReadCostPerMillion,
          cacheCreation: tier.cacheCreationCostPerMillion
        })
      } else {
        await platformAdapter.saveTimeRuleContextTier({
          modelId,
          startTime: data.startTime,
          endTime: data.endTime,
          threshold: tier.threshold,
          input: tier.inputCostPerMillion,
          output: tier.outputCostPerMillion,
          cacheRead: tier.cacheReadCostPerMillion,
          cacheCreation: tier.cacheCreationCostPerMillion
        })
      }
    }
  } else {
    await platformAdapter.addTimePricingRule({
      modelId,
      startTime: data.startTime,
      endTime: data.endTime,
      input: data.input,
      output: data.output,
      cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation,
      label: data.label
    })
    // 保存新增规则的上下文档位
    for (const tier of tiers) {
      await platformAdapter.saveTimeRuleContextTier({
        modelId,
        startTime: data.startTime,
        endTime: data.endTime,
        threshold: tier.threshold,
        input: tier.inputCostPerMillion,
        output: tier.outputCostPerMillion,
        cacheRead: tier.cacheReadCostPerMillion,
        cacheCreation: tier.cacheCreationCostPerMillion
      })
    }
  }
  await platformAdapter.refreshPricing()
  await loadPricingData()
  editingTimeRule.value = null
}

async function onDeleteTimeRule(rule: { id: number; modelId: string; startTime: number; endTime: number }): Promise<void> {
  await platformAdapter.deleteTimePricingRule({
    id: rule.id
  })
  await platformAdapter.refreshPricing()
  await loadPricingData()
}

// 初始化
watch(() => dbStore.hasDatabase, async (val) => {
  if (val) {
    await loadPricingData()
  }
}, { immediate: true })

// 时间弹窗关闭时清理查看状态
watch(showTimeDialog, (val) => {
  if (!val) {
    viewingCloudRule.value = null
    editingTimeRule.value = null
  }
})
</script>

<style scoped>
.pricing-calculator {
  display: flex;
  flex-direction: column;
  min-height: 200px;
}

.pricing-toolbar {
  padding: 4px 0 6px;
  border-bottom: 1px solid var(--border-light);
  margin-bottom: 12px;
}

.toolbar-row {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.filter-group {
  display: flex;
  align-items: center;
  gap: 3px;
}

.filter-label {
  font-size: 11px;
  color: var(--text-tertiary);
  white-space: nowrap;
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
