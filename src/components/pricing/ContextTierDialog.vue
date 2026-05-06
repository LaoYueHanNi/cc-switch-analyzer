<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)">
    <n-card style="width: 300px" :title="isEdit ? '编辑上下文档位' : '添加上下文档位'" :bordered="false" size="small">
      <div class="tier-form">
        <div class="form-row">
          <span class="form-label">边界值 (K tokens)</span>
          <n-input-number v-model:value="threshold" size="tiny" :show-button="false" :min="1" :step="1" style="width: 130px" />
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
import { NModal, NCard, NInputNumber, NButton } from 'naive-ui'

const props = defineProps<{
  show: boolean
  isEdit?: boolean
  initialData?: {
    threshold: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }
}>()

const emit = defineEmits<{
  'update:show': [value: boolean]
  confirm: [data: {
    threshold: number
    input: number
    output: number
    cacheRead: number
    cacheCreation: number
  }]
}>()

const threshold = ref(128)
const input = ref(0)
const output = ref(0)
const cacheRead = ref(0)
const cacheCreation = ref(0)

watch(() => props.show, (val) => {
  if (val && props.initialData) {
    threshold.value = props.initialData.threshold / 1000
    input.value = props.initialData.input
    output.value = props.initialData.output
    cacheRead.value = props.initialData.cacheRead
    cacheCreation.value = props.initialData.cacheCreation
  } else if (val) {
    threshold.value = 128
    input.value = 0
    output.value = 0
    cacheRead.value = 0
    cacheCreation.value = 0
  }
})

function onConfirm(): void {
  emit('confirm', {
    threshold: Math.round(threshold.value * 1000),
    input: input.value,
    output: output.value,
    cacheRead: cacheRead.value,
    cacheCreation: cacheCreation.value
  })
  emit('update:show', false)
}
</script>

<style scoped>
.tier-form {
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
