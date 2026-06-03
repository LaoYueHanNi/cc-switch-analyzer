<template>
  <CompactDialog :show="show" :title="isEdit ? '编辑任务' : '新建任务'" width="420px" @update:show="emit('update:show', $event)">
    <div class="tcd-form">
      <div class="tcd-row">
        <span class="tcd-label">标题</span>
        <CompactInput
          v-model:model-value="form.title"
          placeholder="给任务起个名字"
          :maxlength="60"
        />
        <span class="tcd-count">{{ form.title.length }}/60</span>
      </div>
      <div class="tcd-row">
        <span class="tcd-label">状态</span>
        <CompactSelect
          v-model:model-value="form.status"
          :options="statusOptions"
          :searchable="false"
          placeholder="选择状态"
        />
      </div>
      <div class="tcd-textarea-block">
        <div class="tcd-textarea-head">
          <span class="tcd-label">备注</span>
          <span class="tcd-count">{{ form.description.length }}/500</span>
        </div>
        <textarea
          v-model="form.description"
          class="tcd-textarea"
          placeholder="可选:任务目标、关键节点等"
          :maxlength="500"
        />
      </div>
    </div>
    <template #footer>
      <button class="cd-btn" @click="emit('update:show', false)">取消</button>
      <button class="cd-btn primary" @click="onSave">保存</button>
    </template>
  </CompactDialog>
</template>

<script setup lang="ts">
import { reactive, watch, computed } from 'vue'
import CompactDialog from '@/components/common/CompactDialog.vue'
import CompactInput from '@/components/common/CompactInput.vue'
import CompactSelect from '@/components/common/CompactSelect.vue'
import { TASK_STATUS_OPTIONS, type TaskStatus } from '@/types/task'

const props = defineProps<{
  show: boolean
  initial?: {
    title: string
    description: string
    status: TaskStatus
  } | null
  saving?: boolean
}>()

const emit = defineEmits<{
  'update:show': [v: boolean]
  save: [data: { title: string; description: string; status: TaskStatus }]
}>()

const isEdit = computed(() => !!props.initial)

const form = reactive<{ title: string; description: string; status: TaskStatus }>({
  title: '',
  description: '',
  status: 'todo'
})

const statusOptions = TASK_STATUS_OPTIONS.map(o => ({ label: o.label, value: o.value }))

watch(
  () => [props.show, props.initial],
  ([show]) => {
    if (show) {
      if (props.initial) {
        form.title = props.initial.title
        form.description = props.initial.description
        form.status = props.initial.status
      } else {
        form.title = ''
        form.description = ''
        form.status = 'todo'
      }
    }
  },
  { immediate: true }
)

function onSave() {
  if (!form.title.trim()) return
  emit('save', {
    title: form.title.trim(),
    description: form.description.trim(),
    status: form.status
  })
}
</script>

<style scoped>
.tcd-form { display: flex; flex-direction: column; gap: 8px; }
.tcd-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.tcd-label {
  font-size: 12px;
  color: var(--text-secondary);
  width: 60px;
  flex-shrink: 0;
}
.tcd-count {
  font-size: 10px;
  color: var(--text-faint);
  flex-shrink: 0;
  min-width: 50px;
  text-align: right;
}

.tcd-textarea-block {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.tcd-textarea-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 2px;
}
.tcd-textarea-head .tcd-label {
  width: auto;
}
.tcd-textarea {
  font-size: 11px;
  color: var(--text-primary);
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 3px;
  padding: 4px 6px;
  outline: none;
  transition: border-color 0.15s;
  min-height: 60px;
  max-height: 120px;
  resize: vertical;
  font-family: inherit;
  line-height: 1.5;
  width: 100%;
  box-sizing: border-box;
}
.tcd-textarea:hover { border-color: var(--color-blue); }
.tcd-textarea:focus { border-color: var(--color-blue); }
.tcd-textarea::placeholder { color: var(--text-faint); }

.cd-btn {
  font-size: 11px; padding: 2px 10px; border: 1px solid var(--border-main);
  border-radius: 3px; background: transparent; color: var(--text-primary); cursor: pointer;
}
.cd-btn:hover { border-color: var(--color-blue); color: var(--color-blue); }
.cd-btn.primary { background: var(--color-blue); border-color: var(--color-blue); color: #fff; }
.cd-btn.primary:hover { opacity: 0.85; }
</style>
