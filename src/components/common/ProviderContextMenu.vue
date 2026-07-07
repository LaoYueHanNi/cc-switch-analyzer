<template>
  <Teleport to="body">
    <div v-if="menu.show" class="provider-ctx-overlay" @click="emit('close')" @contextmenu.prevent="emit('close')" />
    <div v-if="menu.show" ref="menuRef" class="provider-ctx-menu" :style="{ left: menu.x + 'px', top: menu.y + 'px' }">
      <div class="provider-ctx-header">选择供应商配置</div>
      <div
        v-for="item in menu.items"
        :key="item.id"
        class="provider-ctx-item"
        @click="emit('select', item.id)"
      >{{ item.name }}</div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import type { ProviderMenuItem } from '@/composables/useProviderContextMenu'

const props = defineProps<{
  menu: { show: boolean; x: number; y: number; items: ProviderMenuItem[] }
  adjustPosition: (el: HTMLElement | null) => void
}>()
const emit = defineEmits<{ select: [providerId: string]; close: [] }>()

const menuRef = ref<HTMLElement | null>(null)

// 显示后按实际渲染高度做一次纵向越界校正
watch(() => props.menu.show, (show) => {
  if (!show) return
  nextTick(() => props.adjustPosition(menuRef.value))
})
</script>

<style scoped>
.provider-ctx-overlay {
  position: fixed; inset: 0; z-index: 9999;
}
.provider-ctx-menu {
  position: fixed; z-index: 10000;
  min-width: 120px; max-width: 220px;
  max-height: 280px; overflow-y: auto;
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 6px;
  box-shadow: var(--shadow-card);
  padding: 3px 0;
  font-size: 11px;
}
.provider-ctx-header {
  padding: 3px 10px;
  font-size: 10px;
  color: var(--text-muted);
  user-select: none;
}
.provider-ctx-item {
  padding: 4px 10px;
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 3px;
  margin: 0 3px;
  transition: background var(--transition-speed);
}
.provider-ctx-item:hover {
  background: var(--bg-hover);
  color: var(--color-blue);
}
</style>
