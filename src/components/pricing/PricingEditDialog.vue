<template>
  <CompactDialog :show="show" :title="modelName + ' — 编辑定价'" @update:show="emit('update:show', $event)">
    <div class="edit-form">
      <div class="edit-row">
        <span class="edit-label">输入单价 /M</span>
        <CompactNumber v-model:model-value="input" :min="0" :step="0.01" width="130px" />
      </div>
      <div class="edit-row">
        <span class="edit-label">输出单价 /M</span>
        <CompactNumber v-model:model-value="output" :min="0" :step="0.01" width="130px" />
      </div>
      <div class="edit-row">
        <span class="edit-label">缓存读取单价 /M</span>
        <CompactNumber v-model:model-value="cacheRead" :min="0" :step="0.01" width="130px" />
      </div>
      <div class="edit-row">
        <span class="edit-label">缓存写入单价 /M</span>
        <CompactNumber v-model:model-value="cacheCreation" :min="0" :step="0.01" width="130px" />
      </div>

      <div v-if="localTiers.length > 0" class="tier-divider">上下文档位</div>
      <div v-for="(tier, idx) in localTiers" :key="idx" class="tier-row">
        <span class="tier-info">>= {{ Math.round(tier.threshold / 1000) }}K &nbsp; {{ formatRate(tier.inputCostPerMillion) }}/M</span>
        <button class="tier-btn" title="编辑" @click="onEditTier(idx)">✎</button>
        <button class="tier-btn" title="删除" @click="onRemoveTier(idx)">✕</button>
      </div>
      <button class="add-tier-btn" @click="onAddTier">+ 添加上下文档位</button>
    </div>
    <template #footer>
      <div class="edit-footer">
        <button v-if="showRestore" class="cd-btn" @click="emit('restore')">恢复默认</button>
        <div class="footer-right">
          <button class="cd-btn" @click="emit('update:show', false)">取消</button>
          <button class="cd-btn primary" @click="onSave">保存</button>
        </div>
      </div>
    </template>
  </CompactDialog>

  <ContextTierDialog
    v-model:show="showTierDialog"
    :is-edit="editingTierIdx !== null"
    :initial-data="editingTierData"
    @confirm="onConfirmTier"
  />
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import CompactDialog from '@/components/common/CompactDialog.vue'
import CompactNumber from '@/components/common/CompactNumber.vue'
import ContextTierDialog from './ContextTierDialog.vue'
import { formatRate } from '@/utils/format'
import { useContextTierEditor } from '@/composables/useContextTierEditor'
import type { ContextTier } from '@/types/pricing'

const props = defineProps<{
  show: boolean
  modelName: string
  currentPricing: { input: number; output: number; cacheRead: number; cacheCreation: number }
  showRestore?: boolean
  contextTiers?: ContextTier[]
}>()

const emit = defineEmits<{
  'update:show': [value: boolean]
  save: [data: { input: number; output: number; cacheRead: number; cacheCreation: number }, tiers: ContextTier[]]
  restore: []
}>()

const input = ref(0)
const output = ref(0)
const cacheRead = ref(0)
const cacheCreation = ref(0)

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
  if (val) {
    input.value = props.currentPricing.input
    output.value = props.currentPricing.output
    cacheRead.value = props.currentPricing.cacheRead
    cacheCreation.value = props.currentPricing.cacheCreation
    localTiers.value = props.contextTiers ? [...props.contextTiers.map(t => ({ ...t }))] : []
  }
  editingTierIdx.value = null
})

function onSave(): void {
  emit('save', {
    input: input.value,
    output: output.value,
    cacheRead: cacheRead.value,
    cacheCreation: cacheCreation.value
  }, [...localTiers.value])
  emit('update:show', false)
}
</script>

<style scoped>
.edit-form { display: flex; flex-direction: column; gap: 6px; }
.edit-row { display: flex; align-items: center; justify-content: space-between; }
.edit-label { font-size: 12px; color: var(--text-secondary); white-space: nowrap; }
.edit-footer { display: flex; align-items: center; justify-content: space-between; width: 100%; }
.footer-right { display: flex; gap: 6px; }
.cd-btn {
  font-size: 11px; padding: 2px 10px; border: 1px solid var(--border-main);
  border-radius: 3px; background: transparent; color: var(--text-primary); cursor: pointer;
}
.cd-btn:hover { border-color: var(--color-blue); color: var(--color-blue); }
.cd-btn.primary { background: var(--color-blue); border-color: var(--color-blue); color: #fff; }
.cd-btn.primary:hover { opacity: 0.85; }
.tier-divider { font-size: 10px; color: var(--text-faint); border-top: 1px solid var(--border-main); padding-top: 4px; margin-top: 2px; }
.tier-row { display: flex; align-items: center; gap: 3px; }
.tier-info { flex: 1; font-size: 10px; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
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
