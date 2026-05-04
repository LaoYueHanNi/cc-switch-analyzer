<template>
  <div class="pricing-edit-form">
    <div class="edit-row">
      <span class="edit-label">输入</span>
      <n-input-number v-model:value="input" size="tiny" :min="0" :step="0.01" style="width: 90px" />
    </div>
    <div class="edit-row">
      <span class="edit-label">输出</span>
      <n-input-number v-model:value="output" size="tiny" :min="0" :step="0.01" style="width: 90px" />
    </div>
    <div class="edit-row">
      <span class="edit-label">缓存读</span>
      <n-input-number v-model:value="cacheRead" size="tiny" :min="0" :step="0.01" style="width: 90px" />
    </div>
    <div class="edit-row">
      <span class="edit-label">缓存写</span>
      <n-input-number v-model:value="cacheCreation" size="tiny" :min="0" :step="0.01" style="width: 90px" />
    </div>
    <div class="edit-actions">
      <n-button size="tiny" type="primary" @click="$emit('save', { input, output, cacheRead, cacheCreation })">保存</n-button>
      <n-button size="tiny" @click="$emit('cancel')">取消</n-button>
      <n-button v-if="showRestore" size="tiny" quaternary @click="$emit('restore')">恢复</n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { NInputNumber, NButton } from 'naive-ui'

const props = defineProps<{
  currentPricing: { input: number; output: number; cacheRead: number; cacheCreation: number }
  showRestore?: boolean
}>()

defineEmits<{
  save: [data: { input: number; output: number; cacheRead: number; cacheCreation: number }]
  cancel: []
  restore: []
}>()

const input = ref(props.currentPricing.input)
const output = ref(props.currentPricing.output)
const cacheRead = ref(props.currentPricing.cacheRead)
const cacheCreation = ref(props.currentPricing.cacheCreation)
</script>

<style scoped>
.pricing-edit-form {
  padding: 4px 0;
}

.edit-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 3px;
}

.edit-label {
  font-size: 11px;
  color: #666;
  white-space: nowrap;
}

.edit-actions {
  display: flex;
  gap: 4px;
  margin-top: 6px;
  justify-content: flex-end;
}
</style>
