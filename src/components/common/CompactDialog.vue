<template>
  <Teleport to="body">
    <div
      v-if="show"
      class="compact-dialog-overlay"
      :style="{ zIndex }"
      @click.self="emit('update:show', false)"
    >
      <div class="compact-dialog" :style="{ width }">
        <div class="compact-dialog-header">
          <div v-if="title" class="compact-dialog-title">{{ title }}</div>
          <div v-else class="compact-dialog-title-spacer" />
          <button
            type="button"
            class="compact-dialog-close"
            title="关闭"
            aria-label="关闭"
            @click="emit('update:show', false)"
          >×</button>
        </div>
        <div class="compact-dialog-body">
          <slot />
        </div>
        <div v-if="$slots.footer" class="compact-dialog-footer">
          <slot name="footer" />
        </div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
withDefaults(defineProps<{
  show: boolean
  title?: string
  width?: string
  /** 叠层顺序；嵌套弹窗应高于底层弹窗 */
  zIndex?: number
}>(), {
  title: '',
  width: '300px',
  zIndex: 10000,
})

const emit = defineEmits<{ 'update:show': [value: boolean] }>()
</script>

<style>
.compact-dialog-overlay {
  position: fixed; inset: 0;
  background: rgba(0,0,0,0.5);
  display: flex; align-items: center; justify-content: center;
}
.compact-dialog {
  background: var(--bg-card);
  border-radius: 8px;
  padding: 16px;
  box-shadow: 0 8px 32px rgba(0,0,0,0.3);
  max-height: 90vh;
  overflow-y: auto;
}
.compact-dialog-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}
.compact-dialog-title {
  flex: 1;
  min-width: 0;
  font-size: 13px; font-weight: 600; color: var(--text-primary);
}
.compact-dialog-title-spacer {
  flex: 1;
}
.compact-dialog-close {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  padding: 0;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-tertiary);
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
}
.compact-dialog-close:hover {
  color: var(--text-primary);
  background: var(--bg-hover, rgba(255, 255, 255, 0.06));
}
.compact-dialog-footer {
  display: flex; gap: 6px; justify-content: flex-end;
  margin-top: 12px;
}
</style>
