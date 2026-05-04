<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)">
    <n-card style="width: 280px" :bordered="false" size="small" :title="modelName + ' — 编辑定价'">
      <div class="edit-form">
        <div class="edit-row">
          <span class="edit-label">输入单价 /M</span>
          <n-input-number v-model:value="input" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 100px" />
        </div>
        <div class="edit-row">
          <span class="edit-label">输出单价 /M</span>
          <n-input-number v-model:value="output" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 100px" />
        </div>
        <div class="edit-row">
          <span class="edit-label">缓存读取单价 /M</span>
          <n-input-number v-model:value="cacheRead" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 100px" />
        </div>
        <div class="edit-row">
          <span class="edit-label">缓存写入单价 /M</span>
          <n-input-number v-model:value="cacheCreation" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 100px" />
        </div>
      </div>
      <template #footer>
        <div class="edit-footer">
          <n-button v-if="showRestore" size="tiny" quaternary @click="$emit('restore')">恢复默认</n-button>
          <div class="footer-right">
            <n-button size="tiny" @click="$emit('update:show', false)">取消</n-button>
            <n-button size="tiny" type="primary" @click="$emit('save', { input, output, cacheRead, cacheCreation })">保存</n-button>
          </div>
        </div>
      </template>
    </n-card>
  </n-modal>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { NModal, NCard, NInputNumber, NButton } from 'naive-ui'

const props = defineProps<{
  show: boolean
  modelName: string
  currentPricing: { input: number; output: number; cacheRead: number; cacheCreation: number }
  showRestore?: boolean
}>()

defineEmits<{
  'update:show': [value: boolean]
  save: [data: { input: number; output: number; cacheRead: number; cacheCreation: number }]
  restore: []
}>()

const input = ref(0)
const output = ref(0)
const cacheRead = ref(0)
const cacheCreation = ref(0)

watch(() => props.show, (val) => {
  if (val) {
    input.value = props.currentPricing.input
    output.value = props.currentPricing.output
    cacheRead.value = props.currentPricing.cacheRead
    cacheCreation.value = props.currentPricing.cacheCreation
  }
})
</script>

<style scoped>
.edit-form {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.edit-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.edit-label {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.edit-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.footer-right {
  display: flex;
  gap: 6px;
}
</style>
