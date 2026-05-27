<template>
  <CompactDialog :show="show" :title="'别名管理 - ' + modelName" width="360px" @update:show="emit('update:show', $event)">
    <div class="alias-form">
      <div class="form-row">
        <CompactInput v-model:model-value="newAlias" placeholder="输入别名" :maxlength="50" @enter="onAdd" width="180px" />
        <button class="cd-btn primary" :disabled="!newAlias.trim()" @click="onAdd">添加</button>
      </div>
      <div v-if="aliases.length === 0" class="empty-hint">暂无别名</div>
      <div v-else class="alias-list">
        <div v-for="alias in aliases" :key="alias" class="alias-tag" :class="{ 'cloud-alias': !props.userAliases.includes(alias) }">
          <span class="alias-text">{{ alias }}</span>
          <button v-if="props.userAliases.includes(alias)" class="delete-btn" @click="onRemove(alias)">✕</button>
        </div>
      </div>
    </div>
  </CompactDialog>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import CompactDialog from '@/components/common/CompactDialog.vue'
import CompactInput from '@/components/common/CompactInput.vue'
import { platformAdapter } from '@/platform'

const props = defineProps<{
  show: boolean
  modelId: string
  modelName: string
  currentAliases: string[]
  userAliases: string[]
}>()

const emit = defineEmits<{
  'update:show': [value: boolean]
  'changed': []
}>()

const aliases = ref<string[]>([])
const newAlias = ref('')

watch(() => props.show, (val) => {
  if (val) {
    aliases.value = [...props.currentAliases]
    newAlias.value = ''
  }
})

async function onAdd(): Promise<void> {
  const name = newAlias.value.trim()
  if (!name || aliases.value.includes(name)) return
  await platformAdapter.addUserAlias(props.modelId, name)
  aliases.value.push(name)
  newAlias.value = ''
  emit('changed')
}

async function onRemove(alias: string): Promise<void> {
  await platformAdapter.removeUserAlias(props.modelId, alias)
  aliases.value = aliases.value.filter(a => a !== alias)
  emit('changed')
}
</script>

<style scoped>
.alias-form { display: flex; flex-direction: column; gap: 8px; }
.form-row { display: flex; align-items: center; gap: 6px; }
.cd-btn {
  font-size: 11px; padding: 2px 10px; border: 1px solid var(--border-main);
  border-radius: 3px; background: transparent; color: var(--text-primary); cursor: pointer;
}
.cd-btn.primary { background: var(--color-blue); border-color: var(--color-blue); color: #fff; }
.cd-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.empty-hint { font-size: 12px; color: var(--text-muted); text-align: center; padding: 12px 0; }
.alias-list { display: flex; flex-wrap: wrap; gap: 4px; max-height: 200px; overflow-y: auto; }
.alias-tag {
  display: inline-flex; align-items: center; gap: 2px;
  padding: 1px 4px 1px 8px; background: var(--bg-hover);
  border: 1px solid var(--border-main); border-radius: 3px; font-size: 12px;
}
.alias-text { color: var(--text-secondary); max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.delete-btn {
  font-size: 10px; color: var(--text-muted); background: none; border: none;
  cursor: pointer; padding: 0 2px;
}
.delete-btn:hover { color: var(--color-cost); }
.cloud-alias { opacity: 0.6; border-style: dashed; }
</style>
