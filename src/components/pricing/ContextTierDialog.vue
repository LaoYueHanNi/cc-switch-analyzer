<template>
  <CompactDialog :show="show" :title="isEdit ? '编辑上下文档位' : '添加上下文档位'" @update:show="emit('update:show', $event)">
    <div class="tier-form">
      <div class="form-row">
        <span class="form-label">边界值 (K tokens)</span>
        <CompactNumber v-model:model-value="threshold" :min="1" :step="1" width="130px" />
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
    </div>
    <template #footer>
      <button class="cd-btn" @click="emit('update:show', false)">取消</button>
      <button class="cd-btn primary" @click="onConfirm">{{ isEdit ? '更新' : '添加' }}</button>
    </template>
  </CompactDialog>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import CompactDialog from '@/components/common/CompactDialog.vue'
import CompactNumber from '@/components/common/CompactNumber.vue'

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
.tier-form { display: flex; flex-direction: column; gap: 6px; }
.form-row { display: flex; align-items: center; justify-content: space-between; }
.form-label { font-size: 12px; color: var(--text-secondary); white-space: nowrap; }
.cd-btn {
  font-size: 11px; padding: 2px 10px; border: 1px solid var(--border-main);
  border-radius: 3px; background: transparent; color: var(--text-primary); cursor: pointer;
}
.cd-btn:hover { border-color: var(--color-blue); color: var(--color-blue); }
.cd-btn.primary { background: var(--color-blue); border-color: var(--color-blue); color: #fff; }
.cd-btn.primary:hover { opacity: 0.85; }
</style>
