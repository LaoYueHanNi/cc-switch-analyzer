<template>
  <div class="slots-editor">
    <div v-if="!disabled" class="slots-header">
      <span class="slots-title">峰时配置</span>
      <button type="button" class="add-btn" @click="addSlot">+ 峰时</button>
    </div>

    <div v-if="!localSlots.length" class="slots-empty">未配置峰时，全天使用上方谷价</div>

    <!-- 只读：摘要展示 -->
    <template v-if="disabled">
      <div v-for="(slot, sIdx) in localSlots" :key="'ro-' + sIdx" class="slot-summary">
        <div class="summary-title">{{ slot.label || '高峰时段' }}</div>
        <div class="summary-row">
          <span class="summary-label">生效日</span>
          <span class="summary-value">{{ formatDaysOfWeek(slot.daysOfWeek) || '每天' }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">时段</span>
          <span class="summary-value">{{ formatWindows(slot.windows) }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">输入 /M</span>
          <span class="summary-value">{{ formatRate(slot.inputCostPerMillion) }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">输出 /M</span>
          <span class="summary-value">{{ formatRate(slot.outputCostPerMillion) }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">缓存读取 /M</span>
          <span class="summary-value">{{ formatRate(slot.cacheReadCostPerMillion) }}</span>
        </div>
        <div class="summary-row">
          <span class="summary-label">缓存写入 /M</span>
          <span class="summary-value">{{ formatRate(slot.cacheCreationCostPerMillion) }}</span>
        </div>
      </div>
    </template>

    <!-- 编辑 -->
    <template v-else>
      <div v-for="(slot, sIdx) in localSlots" :key="'ed-' + sIdx" class="slot-card">
        <div class="form-row">
          <span class="form-label">标签</span>
          <div class="row-right">
            <CompactInput
              v-model:model-value="slot.label"
              placeholder="如：高峰时段"
              width="130px"
            />
            <button type="button" class="tier-btn" title="删除" @click="removeSlot(sIdx)">✕</button>
          </div>
        </div>
        <div v-for="(w, wIdx) in slot.windows" :key="wIdx" class="form-row">
          <span class="form-label">{{ wIdx === 0 ? '时段' : '' }}</span>
          <div class="row-right">
            <div class="time-pair">
              <CompactNumber v-model:model-value="w.startHour" :min="0" :max="23" :step="1" width="44px" />
              <span class="time-sep">:</span>
              <CompactNumber v-model:model-value="w.startMin" :min="0" :max="59" :step="1" width="44px" />
              <span class="time-sep range">~</span>
              <CompactNumber v-model:model-value="w.endHour" :min="0" :max="24" :step="1" width="44px" />
              <span class="time-sep">:</span>
              <CompactNumber v-model:model-value="w.endMin" :min="0" :max="59" :step="1" width="44px" />
            </div>
            <button type="button" class="tier-btn" title="删除时段" @click="removeWindow(sIdx, wIdx)">✕</button>
          </div>
        </div>
        <button type="button" class="add-btn small" @click="addWindow(sIdx)">+ 时段</button>
        <div class="form-row">
          <span class="form-label" title="不勾选 = 每天生效">生效日</span>
          <div class="day-chips">
            <button
              v-for="d in 7"
              :key="d"
              type="button"
              class="day-chip"
              :class="{ active: slot.daysOfWeek.includes(d) }"
              @click="toggleDay(slot, d)"
            >{{ DAY_LABELS[d - 1] }}</button>
            <span v-if="!slot.daysOfWeek.length" class="day-hint">每天</span>
          </div>
        </div>
        <div class="form-row">
          <span class="form-label">输入单价 /M</span>
          <CompactNumber v-model:model-value="slot.inputCostPerMillion" :min="0" :step="0.01" width="130px" />
        </div>
        <div class="form-row">
          <span class="form-label">输出单价 /M</span>
          <CompactNumber v-model:model-value="slot.outputCostPerMillion" :min="0" :step="0.01" width="130px" />
        </div>
        <div class="form-row">
          <span class="form-label">缓存读取单价 /M</span>
          <CompactNumber v-model:model-value="slot.cacheReadCostPerMillion" :min="0" :step="0.01" width="130px" />
        </div>
        <div class="form-row">
          <span class="form-label">缓存写入单价 /M</span>
          <CompactNumber v-model:model-value="slot.cacheCreationCostPerMillion" :min="0" :step="0.01" width="130px" />
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import CompactInput from '@/components/common/CompactInput.vue'
import CompactNumber from '@/components/common/CompactNumber.vue'
import { formatRate } from '@/utils/format'
import { formatDaysOfWeek } from '@/utils/pricing'
import type { DailySlot } from '@/types/pricing'

interface WindowEdit {
  startHour: number
  startMin: number
  endHour: number
  endMin: number
}

interface SlotEdit {
  label: string
  windows: WindowEdit[]
  daysOfWeek: number[]
  inputCostPerMillion: number
  outputCostPerMillion: number
  cacheReadCostPerMillion: number
  cacheCreationCostPerMillion: number
}

const DAY_LABELS = ['一', '二', '三', '四', '五', '六', '日']

const props = defineProps<{
  modelValue: DailySlot[]
  disabled?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: DailySlot[]]
}>()

const localSlots = ref<SlotEdit[]>([])

function pad2(n: number): string {
  return String(n).padStart(2, '0')
}

function formatWindows(windows: WindowEdit[]): string {
  if (!windows.length) return '—'
  return windows
    .map(w => `${pad2(w.startHour)}:${pad2(w.startMin)}–${pad2(w.endHour)}:${pad2(w.endMin)}`)
    .join('、')
}

function toEdit(slots: DailySlot[]): SlotEdit[] {
  return (slots || []).map(s => ({
    label: s.label || '峰时',
    windows: (s.windows || []).map(w => ({
      startHour: Math.floor(w.startMinute / 60),
      startMin: w.startMinute % 60,
      endHour: Math.floor(w.endMinute / 60),
      endMin: w.endMinute % 60
    })),
    daysOfWeek: [...(s.daysOfWeek || [])],
    inputCostPerMillion: s.inputCostPerMillion,
    outputCostPerMillion: s.outputCostPerMillion,
    cacheReadCostPerMillion: s.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: s.cacheCreationCostPerMillion
  }))
}

function toSlots(edits: SlotEdit[]): DailySlot[] {
  return edits.map(s => ({
    label: s.label,
    windows: s.windows.map(w => ({
      startMinute: w.startHour * 60 + w.startMin,
      endMinute: w.endHour * 60 + w.endMin
    })),
    // 空数组 = 每天生效，序列化时省略该字段
    ...(s.daysOfWeek.length ? { daysOfWeek: [...s.daysOfWeek] } : {}),
    inputCostPerMillion: s.inputCostPerMillion,
    outputCostPerMillion: s.outputCostPerMillion,
    cacheReadCostPerMillion: s.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: s.cacheCreationCostPerMillion
  }))
}

watch(() => props.modelValue, (v) => {
  localSlots.value = toEdit(v || [])
}, { immediate: true, deep: true })

watch(localSlots, () => {
  if (props.disabled) return
  emit('update:modelValue', toSlots(localSlots.value))
}, { deep: true })

function addSlot(): void {
  localSlots.value.push({
    label: '高峰时段',
    windows: [{ startHour: 9, startMin: 0, endHour: 12, endMin: 0 }],
    daysOfWeek: [],
    inputCostPerMillion: 0,
    outputCostPerMillion: 0,
    cacheReadCostPerMillion: 0,
    cacheCreationCostPerMillion: 0
  })
}

function removeSlot(idx: number): void {
  localSlots.value.splice(idx, 1)
}

function toggleDay(slot: SlotEdit, day: number): void {
  const idx = slot.daysOfWeek.indexOf(day)
  if (idx >= 0) {
    slot.daysOfWeek.splice(idx, 1)
  } else {
    slot.daysOfWeek.push(day)
  }
}

function addWindow(sIdx: number): void {
  localSlots.value[sIdx].windows.push({ startHour: 14, startMin: 0, endHour: 18, endMin: 0 })
}

function removeWindow(sIdx: number, wIdx: number): void {
  localSlots.value[sIdx].windows.splice(wIdx, 1)
}
</script>

<style scoped>
.slots-editor { margin-top: 4px; display: flex; flex-direction: column; gap: 6px; }
.slots-header { display: flex; align-items: center; justify-content: space-between; }
.slots-title { font-size: 12px; color: var(--text-secondary); }
.slots-empty { font-size: 11px; color: var(--text-tertiary); }

.slot-summary {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 6px 0 2px;
}
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

.slot-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-top: 2px;
}
.form-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 24px;
}
.form-label {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}
.row-right {
  display: flex;
  align-items: center;
  gap: 4px;
}
.time-pair {
  display: flex;
  align-items: center;
  gap: 2px;
}
.time-sep {
  font-size: 12px;
  color: var(--text-secondary);
}
.time-sep.range { margin: 0 4px; }

.day-chips {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-wrap: wrap;
}
.day-chip {
  width: 22px; height: 18px; font-size: 11px; line-height: 1;
  border: 1px solid var(--border-main); background: transparent;
  color: var(--text-secondary); cursor: pointer; border-radius: 3px; padding: 0;
}
.day-chip:hover { background: var(--bg-hover); }
.day-chip.active {
  border-color: var(--color-blue);
  color: var(--color-blue);
}
.day-hint {
  font-size: 11px;
  color: var(--text-faint);
  margin-left: 2px;
}

.add-btn {
  font-size: 11px; padding: 1px 8px; border: 1px dashed var(--border-main);
  border-radius: 3px; background: transparent; color: var(--text-secondary); cursor: pointer;
}
.add-btn.small { align-self: flex-end; }
.tier-btn {
  width: 18px; height: 18px; border: none; background: none; font-size: 10px;
  color: var(--text-faint); cursor: pointer; border-radius: 2px; padding: 0;
}
.tier-btn:hover { color: var(--text-tertiary); background: var(--bg-hover); }
</style>
