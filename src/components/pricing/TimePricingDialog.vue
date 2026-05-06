<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)">
    <n-card style="width: 300px" :title="isEdit ? '编辑时间定价' : '添加时间定价'" :bordered="false" size="small">
      <div class="time-form">
        <div class="form-row">
          <span class="form-label">标签</span>
          <n-input v-model:value="label" size="tiny" placeholder="如：限时折扣" style="width: 150px" />
        </div>
        <div class="form-row">
          <span class="form-label">起始日期</span>
          <n-date-picker v-model:value="startDate" type="date" size="small" style="width: 150px" />
        </div>
        <div class="form-row">
          <span class="form-label">结束日期</span>
          <n-date-picker v-model:value="endDate" type="date" size="small" style="width: 150px" />
        </div>
        <div class="form-row">
          <span class="form-label">输入单价 /M</span>
          <n-input-number v-model:value="input" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 130px" />
        </div>
        <div class="form-row">
          <span class="form-label">输出单价 /M</span>
          <n-input-number v-model:value="output" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 130px" />
        </div>
        <div class="form-row">
          <span class="form-label">缓存读取单价 /M</span>
          <n-input-number v-model:value="cacheRead" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 130px" />
        </div>
        <div class="form-row">
          <span class="form-label">缓存写入单价 /M</span>
          <n-input-number v-model:value="cacheCreation" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 130px" />
        </div>

        <!-- 上下文定价档位 -->
        <template v-if="localTiers.length > 0">
          <div class="tier-divider">上下文档位</div>
          <div v-for="(tier, idx) in localTiers" :key="idx" class="tier-row">
            <span class="tier-info">>= {{ Math.round(tier.threshold / 1000) }}K &nbsp; {{ formatRate(tier.inputCostPerMillion) }}/M</span>
            <n-button size="tiny" quaternary class="tier-btn" @click="onEditTier(idx)">
              <template #icon><n-icon size="11"><create-outline /></n-icon></template>
            </n-button>
            <n-button size="tiny" quaternary class="tier-btn" @click="onRemoveTier(idx)">
              <template #icon><n-icon size="11"><trash-outline /></n-icon></template>
            </n-button>
          </div>
        </template>
        <n-button size="tiny" quaternary class="add-tier-btn" @click="onAddTier">
          <template #icon><n-icon size="11"><add-outline /></n-icon></template>
          添加上下文档位
        </n-button>

        <div class="form-actions">
          <n-button size="tiny" @click="$emit('update:show', false)">取消</n-button>
          <n-button size="tiny" type="primary" @click="onConfirm">{{ isEdit ? '更新' : '添加' }}</n-button>
        </div>
      </div>
    </n-card>
  </n-modal>

  <!-- 上下文档位编辑子弹窗 -->
  <ContextTierDialog
    v-model:show="showTierDialog"
    :is-edit="editingTierIdx !== null"
    :initial-data="editingTierData"
    @confirm="onConfirmTier"
  />
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { NModal, NCard, NInput, NDatePicker, NInputNumber, NButton, NIcon } from 'naive-ui'
import { CreateOutline, TrashOutline, AddOutline } from '@vicons/ionicons5'
import ContextTierDialog from './ContextTierDialog.vue'
import { formatRate } from '@/utils/format'
import type { ContextTier } from '@/types/pricing'

const props = defineProps<{
  show: boolean
  isEdit?: boolean
  initialData?: {
    label: string
    startTime: number
    endTime: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }
  contextTiers?: ContextTier[]
}>()

const emit = defineEmits<{
  'update:show': [value: boolean]
  confirm: [data: {
    label: string
    startTime: number
    endTime: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }, tiers: ContextTier[]]
}>()

const label = ref('')
const startDate = ref<number | null>(Date.now())
const endDate = ref<number | null>(Date.now() + 7 * 86400 * 1000)
const input = ref(0)
const output = ref(0)
const cacheRead = ref(0)
const cacheCreation = ref(0)

// 本地维护的档位列表（编辑模式下可增删改）
const localTiers = ref<ContextTier[]>([])

// 档位子弹窗
const showTierDialog = ref(false)
const editingTierIdx = ref<number | null>(null)
const editingTierData = ref<{ threshold: number; input: number; output: number; cacheRead: number; cacheCreation: number } | undefined>(undefined)

watch(() => props.show, (val) => {
  if (val && props.initialData) {
    label.value = props.initialData.label
    startDate.value = props.initialData.startTime * 1000
    endDate.value = props.initialData.endTime * 1000
    input.value = props.initialData.input
    output.value = props.initialData.output
    cacheRead.value = props.initialData.cacheRead
    cacheCreation.value = props.initialData.cacheCreation
    localTiers.value = props.contextTiers ? [...props.contextTiers.map(t => ({ ...t }))] : []
  } else if (val) {
    label.value = ''
    startDate.value = Date.now()
    endDate.value = Date.now() + 7 * 86400 * 1000
    input.value = 0
    output.value = 0
    cacheRead.value = 0
    cacheCreation.value = 0
    localTiers.value = []
  }
  editingTierIdx.value = null
})

function onAddTier(): void {
  editingTierIdx.value = null
  editingTierData.value = undefined
  showTierDialog.value = true
}

function onEditTier(idx: number): void {
  const tier = localTiers.value[idx]
  editingTierIdx.value = idx
  editingTierData.value = {
    threshold: tier.threshold,
    input: tier.inputCostPerMillion,
    output: tier.outputCostPerMillion,
    cacheRead: tier.cacheReadCostPerMillion,
    cacheCreation: tier.cacheCreationCostPerMillion
  }
  showTierDialog.value = true
}

function onRemoveTier(idx: number): void {
  localTiers.value.splice(idx, 1)
}

function onConfirmTier(data: { threshold: number; input: number; output: number; cacheRead: number; cacheCreation: number }): void {
  const tier: ContextTier = {
    threshold: data.threshold,
    inputCostPerMillion: data.input,
    outputCostPerMillion: data.output,
    cacheReadCostPerMillion: data.cacheRead,
    cacheCreationCostPerMillion: data.cacheCreation
  }
  if (editingTierIdx.value !== null) {
    localTiers.value[editingTierIdx.value] = tier
  } else {
    localTiers.value.push(tier)
  }
  editingTierIdx.value = null
}

function onConfirm(): void {
  if (!startDate.value || !endDate.value) return
  emit('confirm', {
    label: label.value,
    startTime: Math.floor(startDate.value / 1000),
    endTime: Math.floor(endDate.value / 1000),
    input: input.value,
    output: output.value,
    cacheRead: cacheRead.value,
    cacheCreation: cacheCreation.value
  }, [...localTiers.value])
  emit('update:show', false)
}
</script>

<style scoped>
.time-form {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.form-label {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.form-actions {
  display: flex;
  gap: 6px;
  justify-content: flex-end;
  margin-top: 8px;
}

.tier-divider {
  font-size: 10px;
  color: var(--text-faint);
  border-top: 1px solid var(--border-main);
  padding-top: 4px;
  margin-top: 2px;
}

.tier-row {
  display: flex;
  align-items: center;
  gap: 3px;
}

.tier-info {
  flex: 1;
  font-size: 10px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tier-btn {
  padding: 0 !important;
  min-width: 18px !important;
  height: 18px !important;
  color: var(--text-faint) !important;
}
.tier-btn:hover {
  color: var(--text-tertiary) !important;
}

.add-tier-btn {
  font-size: 10px;
}
</style>
