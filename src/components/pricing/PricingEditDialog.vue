<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)">
    <n-card style="width: 300px" :bordered="false" size="small" :title="modelName + ' — 编辑定价'">
      <div class="edit-form">
        <div class="edit-row">
          <span class="edit-label">输入单价 /M</span>
          <n-input-number v-model:value="input" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 130px" />
        </div>
        <div class="edit-row">
          <span class="edit-label">输出单价 /M</span>
          <n-input-number v-model:value="output" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 130px" />
        </div>
        <div class="edit-row">
          <span class="edit-label">缓存读取单价 /M</span>
          <n-input-number v-model:value="cacheRead" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 130px" />
        </div>
        <div class="edit-row">
          <span class="edit-label">缓存写入单价 /M</span>
          <n-input-number v-model:value="cacheCreation" size="tiny" :show-button="false" :min="0" :step="0.01" style="width: 130px" />
        </div>

        <!-- 上下文定价档位 -->
        <div v-if="localTiers.length > 0" class="tier-divider">上下文档位</div>
        <div v-for="(tier, idx) in localTiers" :key="idx" class="tier-row">
          <span class="tier-info">>= {{ Math.round(tier.threshold / 1000) }}K &nbsp; {{ formatRate(tier.inputCostPerMillion) }}/M</span>
          <n-button size="tiny" quaternary class="tier-btn" @click="onEditTier(idx)">
            <template #icon><n-icon size="11"><create-outline /></n-icon></template>
          </n-button>
          <n-button size="tiny" quaternary class="tier-btn" @click="onRemoveTier(idx)">
            <template #icon><n-icon size="11"><trash-outline /></n-icon></template>
          </n-button>
        </div>
        <n-button size="tiny" quaternary class="add-tier-btn" @click="onAddTier">
          <template #icon><n-icon size="11"><add-outline /></n-icon></template>
          添加上下文档位
        </n-button>
      </div>
      <template #footer>
        <div class="edit-footer">
          <n-button v-if="showRestore" size="tiny" quaternary @click="$emit('restore')">恢复默认</n-button>
          <div class="footer-right">
            <n-button size="tiny" @click="$emit('update:show', false)">取消</n-button>
            <n-button size="tiny" type="primary" @click="onSave">保存</n-button>
          </div>
        </div>
      </template>
    </n-card>
  </n-modal>

  <!-- 上下文档位子弹窗 -->
  <ContextTierDialog
    v-model:show="showTierDialog"
    :is-edit="editingTierIdx !== null"
    :initial-data="editingTierData"
    @confirm="onConfirmTier"
  />
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { NModal, NCard, NInputNumber, NButton, NIcon } from 'naive-ui'
import { CreateOutline, TrashOutline, AddOutline } from '@vicons/ionicons5'
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

.tier-divider {
  font-size: 10px;
  color: var(--text-faint);
  border-top: 1px solid var(--border-main);
  padding-top: 4px;
  margin-top: 2px;
}

.tier-row {
  display: flex;
  align-items: center;
  gap: 3px;
}

.tier-info {
  flex: 1;
  font-size: 10px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tier-btn {
  padding: 0 !important;
  min-width: 18px !important;
  height: 18px !important;
  color: var(--text-faint) !important;
}
.tier-btn:hover {
  color: var(--text-tertiary) !important;
}

.add-tier-btn {
  font-size: 10px;
}
</style>
