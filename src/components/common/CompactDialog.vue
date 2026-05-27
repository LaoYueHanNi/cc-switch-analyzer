<template>
  <Teleport to="body">
    <div v-if="show" class="compact-dialog-overlay" @click.self="emit('update:show', false)">
      <div class="compact-dialog" :style="{ width }">
        <div v-if="title" class="compact-dialog-title">{{ title }}</div>
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
}>(), {
  title: '',
  width: '300px',
})

const emit = defineEmits<{ 'update:show': [value: boolean] }>()
</script>

<style>
.compact-dialog-overlay {
  position: fixed; inset: 0; z-index: 10000;
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
.compact-dialog-title {
  font-size: 13px; font-weight: 600; color: var(--text-primary);
  margin-bottom: 12px;
}
.compact-dialog-footer {
  display: flex; gap: 6px; justify-content: flex-end;
  margin-top: 12px;
}
</style>
