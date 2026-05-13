<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)">
    <n-card style="width: 360px" :title="'别名管理 - ' + modelName" :bordered="false" size="small">
      <div class="alias-form">
        <div class="form-row">
          <n-input
            v-model:value="newAlias"
            size="tiny"
            placeholder="输入别名"
            style="width: 180px"
            :maxlength="50"
            @keyup.enter="onAdd"
          />
          <n-button size="tiny" type="primary" :disabled="!newAlias.trim()" @click="onAdd">添加</n-button>
        </div>
        <div v-if="aliases.length === 0" class="empty-hint">暂无别名</div>
        <div v-else class="alias-list">
          <div v-for="alias in aliases" :key="alias" class="alias-tag" :class="{ 'cloud-alias': !props.userAliases.includes(alias) }">
            <span class="alias-text">{{ alias }}</span>
            <n-button v-if="props.userAliases.includes(alias)" text size="tiny" class="delete-btn" @click="onRemove(alias)">
              <n-icon size="12"><close-outline /></n-icon>
            </n-button>
          </div>
        </div>
      </div>
    </n-card>
  </n-modal>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { NModal, NCard, NInput, NButton, NIcon } from 'naive-ui'
import { CloseOutline } from '@vicons/ionicons5'
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

async function onAdd(): void {
  const name = newAlias.value.trim()
  if (!name || aliases.value.includes(name)) return
  await platformAdapter.addUserAlias(props.modelId, name)
  aliases.value.push(name)
  newAlias.value = ''
  emit('changed')
}

async function onRemove(alias: string): void {
  await platformAdapter.removeUserAlias(props.modelId, alias)
  aliases.value = aliases.value.filter(a => a !== alias)
  emit('changed')
}
</script>

<style scoped>
.alias-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.form-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.empty-hint {
  font-size: 12px;
  color: var(--text-muted);
  text-align: center;
  padding: 12px 0;
}
.alias-list {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  max-height: 200px;
  overflow-y: auto;
}
.alias-tag {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 1px 4px 1px 8px;
  background: var(--bg-hover);
  border: 1px solid var(--border-main);
  border-radius: 3px;
  font-size: 12px;
}
.alias-text {
  color: var(--text-secondary);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.delete-btn {
  color: var(--text-muted);
  padding: 0 2px;
}
.delete-btn:hover {
  color: var(--color-cost);
}
.cloud-alias {
  opacity: 0.6;
  border-style: dashed;
}
</style>
