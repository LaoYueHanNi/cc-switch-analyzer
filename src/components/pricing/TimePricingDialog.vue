<template>
  <CompactDialog :show="show" :title="dialogTitle" @update:show="emit('update:show', $event)">
    <div class="time-form">
      <!-- 只读：与峰时摘要同一套纯文本行 -->
      <template v-if="isReadonly">
        <div class="summary-title">{{ label || '谷价' }}</div>
        <div class="summary-row">
          <span class="summary-label">时间范围</span>
          <span class="summary-value">{{ dateRangeText }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">输入 /M</span>
          <span class="summary-value">{{ formatRate(input) }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">输出 /M</span>
          <span class="summary-value">{{ formatRate(output) }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">缓存读取 /M</span>
          <span class="summary-value">{{ formatRate(cacheRead) }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">缓存写入 /M</span>
          <span class="summary-value">{{ formatRate(cacheCreation) }}</span>
        </div>
      </template>

      <!-- 编辑：表单控件 -->
      <template v-else>
        <div class="form-row">
          <span class="form-label">标签</span>
          <CompactInput v-model:model-value="label" placeholder="如：限时折扣" width="150px" />
        </div>
        <div class="form-row">
          <span class="form-label">时间范围</span>
          <CompactDateRange :value="dateRange" @update:value="onDateRangeChange" />
        </div>
        <div class="form-row">
          <span class="form-label">输入单价 /M</span>
          <CompactNumber v-model:model-value="input" :min="0" :step="0.01" width="130px" />
        </div>
        <div class="form-row">
          <span class="form-label">输出单价 /M</span>
          <CompactNumber v-model:model-value="output" :min="0" :step="0.01" width="130px" />
        </div>
        <div class="form-row">
          <span class="form-label">缓存读取单价 /M</span>
          <CompactNumber v-model:model-value="cacheRead" :min="0" :step="0.01" width="130px" />
        </div>
        <div class="form-row">
          <span class="form-label">缓存写入单价 /M</span>
          <CompactNumber v-model:model-value="cacheCreation" :min="0" :step="0.01" width="130px" />
        </div>
      </template>

      <div class="tier-divider">峰时价（其余时间用上方谷价）</div>
      <DailySlotsEditor v-model="localDailySlots" :disabled="!!isReadonly" />

      <template v-if="localTiers.length > 0">
        <div class="tier-divider">上下文档位</div>
        <div v-for="(tier, idx) in localTiers" :key="idx" class="tier-row">
          <span class="tier-info">
            >= {{ Math.round(tier.threshold / 1000) }}K &nbsp;
            <span class="tier-rate" :style="{ color: 'var(--color-purple)' }">{{ formatRate(tier.inputCostPerMillion) }}</span>
            <span class="tier-rate" :style="{ color: 'var(--color-orange)' }">{{ formatRate(tier.outputCostPerMillion) }}</span>
            <span class="tier-rate" :style="{ color: 'var(--color-blue)' }">{{ formatRate(tier.cacheReadCostPerMillion) }}</span>
            <span class="tier-rate" :style="{ color: 'var(--color-dark-orange)' }">{{ formatRate(tier.cacheCreationCostPerMillion) }}</span>
            <span v-if="tier.dailySlots?.length" class="tier-rate">峰 {{ formatDailySlotsSummary(tier.dailySlots) }}</span>
          </span>
          <template v-if="!isReadonly">
            <button class="tier-btn" title="编辑" @click="onEditTier(idx)">✎</button>
            <button class="tier-btn" title="删除" @click="onRemoveTier(idx)">✕</button>
          </template>
        </div>
      </template>
      <button v-if="!isReadonly" class="add-tier-btn" @click="onAddTier">+ 添加上下文档位</button>

      <div v-if="!isReadonly" class="form-actions">
        <button class="cd-btn" @click="emit('update:show', false)">取消</button>
        <button class="cd-btn primary" @click="onConfirm">{{ isEdit ? '更新' : '添加' }}</button>
      </div>
    </div>
  </CompactDialog>

  <ContextTierDialog
    v-model:show="showTierDialog"
    :is-edit="editingTierIdx !== null"
    :initial-data="editingTierData"
    @confirm="onConfirmTier"
  />
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import CompactDialog from '@/components/common/CompactDialog.vue'
import CompactInput from '@/components/common/CompactInput.vue'
import CompactNumber from '@/components/common/CompactNumber.vue'
import CompactDateRange from '@/components/common/CompactDateRange.vue'
import ContextTierDialog from './ContextTierDialog.vue'
import DailySlotsEditor from './DailySlotsEditor.vue'
import { formatRate } from '@/utils/format'
import { formatDailySlotsSummary } from '@/utils/pricing'
import { useContextTierEditor } from '@/composables/useContextTierEditor'
import type { ContextTier, DailySlot } from '@/types/pricing'

const props = defineProps<{
  show: boolean
  isEdit?: boolean
  readonly?: boolean
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
  dailySlots?: DailySlot[]
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
    dailySlots: DailySlot[]
  }, tiers: ContextTier[]]
}>()

const label = ref('')
const startDate = ref<number | null>(Date.now())
const endDate = ref<number | null>(Date.now() + 7 * 86400 * 1000)
const input = ref(0)
const output = ref(0)
const cacheRead = ref(0)
const cacheCreation = ref(0)
const localDailySlots = ref<DailySlot[]>([])
const isReadonly = ref(false)

const dialogTitle = computed(() => {
  if (isReadonly.value) {
    return label.value === '常驻价' ? '查看常驻价' : '查看时间定价'
  }
  return props.isEdit ? '编辑时间定价' : '添加时间定价'
})

const dateRange = computed<[number, number] | null>(() => {
  if (startDate.value != null && endDate.value != null) return [startDate.value, endDate.value]
  return null
})

function formatLocalDate(ms: number): string {
  const d = new Date(ms)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

const dateRangeText = computed(() => {
  if (startDate.value == null || endDate.value == null) return '—'
  return `${formatLocalDate(startDate.value)} ~ ${formatLocalDate(endDate.value)}`
})

function onDateRangeChange(val: [number, number] | null) {
  if (val) {
    startDate.value = val[0]
    endDate.value = val[1]
  } else {
    startDate.value = null
    endDate.value = null
  }
}

const {
  localTiers,
  showTierDialog,
  editingTierIdx,
  editingTierData,
  onAddTier,
  onEditTier,
  onRemoveTier,
  onConfirmTier
} = useContextTierEditor()

watch(() => props.show, (val) => {
  if (val && props.initialData) {
    isReadonly.value = props.readonly || false
    label.value = props.initialData.label
    startDate.value = props.initialData.startTime * 1000
    endDate.value = props.initialData.endTime * 1000
    input.value = props.initialData.input
    output.value = props.initialData.output
    cacheRead.value = props.initialData.cacheRead
    cacheCreation.value = props.initialData.cacheCreation
    localTiers.value = props.contextTiers ? [...props.contextTiers.map(t => ({ ...t, dailySlots: [...(t.dailySlots || [])] }))] : []
    localDailySlots.value = props.dailySlots ? props.dailySlots.map(s => ({ ...s, windows: [...(s.windows || [])] })) : []
  } else if (val) {
    isReadonly.value = props.readonly || false
    label.value = ''
    startDate.value = Date.now()
    endDate.value = Date.now() + 7 * 86400 * 1000
    input.value = 0
    output.value = 0
    cacheRead.value = 0
    cacheCreation.value = 0
    localTiers.value = []
    localDailySlots.value = []
  }
  editingTierIdx.value = null
})

function onConfirm(): void {
  if (!startDate.value || !endDate.value) return
  emit('confirm', {
    label: label.value,
    startTime: Math.floor(startDate.value / 1000),
    endTime: Math.floor(endDate.value / 1000),
    input: input.value,
    output: output.value,
    cacheRead: cacheRead.value,
    cacheCreation: cacheCreation.value,
    dailySlots: [...localDailySlots.value]
  }, [...localTiers.value])
  emit('update:show', false)
}
</script>

<style scoped>
.time-form { display: flex; flex-direction: column; gap: 6px; }
.form-row { display: flex; align-items: center; justify-content: space-between; }
.form-label { font-size: 12px; color: var(--text-secondary); white-space: nowrap; }
.summary-title {
  font-size: 12px;
  color: var(--text-primary);
  font-weight: 500;
}
.summary-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.summary-label {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}
.summary-value {
  font-size: 12px;
  color: var(--text-primary);
  text-align: right;
}
.form-actions { display: flex; gap: 6px; justify-content: flex-end; margin-top: 8px; }
.cd-btn {
  font-size: 11px; padding: 2px 10px; border: 1px solid var(--border-main);
  border-radius: 3px; background: transparent; color: var(--text-primary); cursor: pointer;
}
.cd-btn:hover { border-color: var(--color-blue); color: var(--color-blue); }
.cd-btn.primary { background: var(--color-blue); border-color: var(--color-blue); color: #fff; }
.tier-divider { font-size: 10px; color: var(--text-faint); border-top: 1px solid var(--border-main); padding-top: 4px; margin-top: 2px; }
.tier-row { display: flex; align-items: center; gap: 3px; }
.tier-info { flex: 1; font-size: 10px; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.tier-rate { font-size: 10px; margin-left: 3px; }
.tier-btn {
  width: 18px; height: 18px; border: none; background: none; font-size: 10px;
  color: var(--text-faint); cursor: pointer; border-radius: 2px; padding: 0;
}
.tier-btn:hover { color: var(--text-tertiary); background: var(--bg-hover); }
.add-tier-btn {
  font-size: 10px; border: none; background: none; color: var(--color-blue);
  cursor: pointer; padding: 2px 0;
}
.add-tier-btn:hover { opacity: 0.7; }
</style>
