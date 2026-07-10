<template>
  <div class="compact-select" ref="triggerRef">
    <div class="compact-select__trigger" @click="toggle">
      <span v-if="!modelValue" class="compact-select__placeholder">{{ placeholder }}</span>
      <span v-else class="compact-select__value">{{ currentLabel }}</span>
      <span v-if="modelValue && clearable" class="compact-select__clear" @click.stop="emit('update:modelValue', '')">✕</span>
      <span v-else class="compact-select__arrow">▾</span>
    </div>
  </div>
  <Teleport to="body">
    <div v-if="open" class="compact-select-overlay" @click="open = false" @contextmenu.prevent="open = false" />
    <div v-if="open" class="compact-select-panel" :style="panelStyle">
      <div v-if="searchable" class="compact-select-search">
        <input
          ref="searchRef"
          v-model="search"
          class="compact-select-search__input"
          placeholder="搜索..."
          @keydown.escape.stop="open = false"
        />
      </div>
      <div class="compact-select-scroll">
        <div
          v-for="opt in filteredOptions"
          :key="opt.value"
          class="compact-select-item"
          :class="{ active: opt.value === modelValue }"
          @click="onSelect(opt.value)"
        >{{ opt.label }}</div>
        <div v-if="filteredOptions.length === 0" class="compact-select-empty">无匹配项</div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, watch, onBeforeUnmount, onMounted } from 'vue'

const props = withDefaults(defineProps<{
  modelValue: string
  options: { label: string; value: string }[]
  placeholder?: string
  clearable?: boolean
  searchable?: boolean
}>(), {
  placeholder: '请选择',
  clearable: false,
  searchable: true,
})

const emit = defineEmits<{ 'update:modelValue': [value: string] }>()

const open = ref(false)
const search = ref('')
const triggerRef = ref<HTMLElement>()
const searchRef = ref<HTMLInputElement>()
const panelStyle = ref<Record<string, string>>({})

const currentLabel = computed(() =>
  props.options.find(o => o.value === props.modelValue)?.label ?? props.modelValue
)

const filteredOptions = computed(() => {
  if (!search.value) return props.options
  const q = search.value.toLowerCase()
  return props.options.filter(o => o.label.toLowerCase().includes(q) || o.value.toLowerCase().includes(q))
})

function toggle() {
  if (open.value) { open.value = false; return }
  search.value = ''
  open.value = true
  nextTick(() => {
    positionPanel()
    searchRef.value?.focus()
  })
}

let measureCanvas: HTMLCanvasElement | null = null

function measureTextWidth(text: string): number {
  if (!measureCanvas) measureCanvas = document.createElement('canvas')
  const ctx = measureCanvas.getContext('2d')
  if (!ctx) return text.length * 7
  ctx.font = '600 11px system-ui, -apple-system, "Segoe UI", sans-serif'
  return ctx.measureText(text).width
}

function getPanelContentWidth(): number {
  const labels = filteredOptions.value.map(o => o.label)
  let maxW = measureTextWidth('搜索...')
  for (const label of labels) {
    maxW = Math.max(maxW, measureTextWidth(label))
  }
  const scrollbar = labels.length > 7 ? 12 : 0
  return maxW + 16 + scrollbar
}

function positionPanel() {
  const el = triggerRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const zoom = parseFloat(getComputedStyle(document.body).zoom) || 1
  const max_x = window.innerWidth / zoom - 8
  const max_y = window.innerHeight / zoom - 8
  let x = rect.left / zoom
  let y = (rect.bottom + 2) / zoom
  const tw = rect.width / zoom
  const contentW = getPanelContentWidth()
  const pw = Math.min(Math.max(tw, contentW, 80), 420, max_x - x)
  const ph = Math.min(filteredOptions.value.length * 22 + 28, 220)
  if (x + pw > max_x) x = Math.max(8, max_x - pw)
  if (y + ph > max_y) y = Math.max(8, rect.top / zoom - ph - 2)
  panelStyle.value = { left: x + 'px', top: y + 'px', width: pw + 'px' }
}

function onSelect(val: string) {
  open.value = false
  emit('update:modelValue', val)
}

watch(filteredOptions, () => { if (open.value) positionPanel() })

function onGlobalKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) open.value = false
}

onMounted(() => window.addEventListener('keydown', onGlobalKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onGlobalKeydown))
</script>

<style>
.compact-select-overlay {
  position: fixed; inset: 0; z-index: 10001;
}
.compact-select-panel {
  position: fixed; z-index: 10002;
  min-width: 80px;
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 4px;
  box-shadow: 0 2px 8px rgba(0,0,0,0.12);
  padding: 2px 0;
}
.compact-select-search {
  padding: 2px 4px;
  border-bottom: 1px solid var(--border-light);
}
.compact-select-search__input {
  width: 100%;
  border: none;
  outline: none;
  background: transparent;
  font-size: 11px;
  color: var(--text-primary);
  padding: 2px 4px;
}
.compact-select-search__input::placeholder {
  color: var(--text-faint);
}
.compact-select-scroll {
  max-height: 180px;
  overflow-y: auto;
}
.compact-select-scroll::-webkit-scrollbar { width: 3px; }
.compact-select-scroll::-webkit-scrollbar-thumb { background: var(--border-main); border-radius: 2px; }
.compact-select-item {
  padding: 2px 8px;
  font-size: 11px;
  color: var(--text-primary);
  cursor: pointer;
  white-space: nowrap;
  transition: background 0.15s;
}
.compact-select-item:hover {
  background: var(--bg-hover);
  color: var(--color-blue);
}
.compact-select-item.active {
  color: var(--color-blue);
  font-weight: 600;
}
.compact-select-empty {
  padding: 4px 8px;
  font-size: 11px;
  color: var(--text-faint);
}
</style>

<style scoped>
.compact-select {
  display: inline-block;
}
.compact-select__trigger {
  display: inline-flex; align-items: center; gap: 2px;
  font-size: 11px;
  color: var(--text-secondary);
  cursor: pointer;
  padding: 1px 4px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: var(--bg-card);
  min-width: 60px;
  max-width: 220px;
  height: 20px;
  line-height: 20px;
  transition: border-color 0.15s;
}
.compact-select__trigger:hover {
  border-color: var(--color-blue);
}
.compact-select__placeholder {
  flex: 1;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.compact-select__value {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.compact-select__clear {
  font-size: 10px;
  color: var(--text-muted);
  flex-shrink: 0;
}
.compact-select__clear:hover {
  color: var(--text-primary);
}
.compact-select__arrow {
  font-size: 10px;
  color: var(--text-muted);
  flex-shrink: 0;
}
</style>
