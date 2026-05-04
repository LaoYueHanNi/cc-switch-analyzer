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
        <div class="form-actions">
          <n-button size="tiny" @click="$emit('update:show', false)">取消</n-button>
          <n-button size="tiny" type="primary" @click="onConfirm">{{ isEdit ? '更新' : '添加' }}</n-button>
        </div>
      </div>
    </n-card>
  </n-modal>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { NModal, NCard, NInput, NDatePicker, NInputNumber, NButton } from 'naive-ui'

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
  }]
}>()

const label = ref('')
const startDate = ref<number | null>(Date.now())
const endDate = ref<number | null>(Date.now() + 7 * 86400 * 1000)
const input = ref(0)
const output = ref(0)
const cacheRead = ref(0)
const cacheCreation = ref(0)

watch(() => props.show, (val) => {
  if (val && props.initialData) {
    label.value = props.initialData.label
    startDate.value = props.initialData.startTime * 1000
    endDate.value = props.initialData.endTime * 1000
    input.value = props.initialData.input
    output.value = props.initialData.output
    cacheRead.value = props.initialData.cacheRead
    cacheCreation.value = props.initialData.cacheCreation
  } else if (val) {
    label.value = ''
    startDate.value = Date.now()
    endDate.value = Date.now() + 7 * 86400 * 1000
    input.value = 0
    output.value = 0
    cacheRead.value = 0
    cacheCreation.value = 0
  }
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
    cacheCreation: cacheCreation.value
  })
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
</style>
