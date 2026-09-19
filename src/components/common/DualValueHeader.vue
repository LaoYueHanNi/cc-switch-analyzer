<template>
  <span class="dual-value-header" :title="title">
    <span
      class="dual-item"
      :class="{ 'is-dimmed': !showA }"
      :title="showA ? `点击隐藏${labelA}` : `点击显示${labelA}`"
      @click="toggleA"
    >
      {{ labelA }}
    </span>
    <span class="dual-separator" :class="{ 'is-dimmed': !showA && !showB }">{{ separator }}</span>
    <span
      class="dual-item"
      :class="{ 'is-dimmed': !showB }"
      :title="showB ? `点击隐藏${labelB}` : `点击显示${labelB}`"
      @click="toggleB"
    >
      {{ labelB }}
    </span>
    <span class="dual-suffix" v-if="suffix">{{ suffix }}</span>
  </span>
</template>

<script setup lang="ts">
defineOptions({ name: 'DualValueHeader' })

const props = withDefaults(
  defineProps<{
    labelA?: string
    labelB?: string
    suffix?: string
    separator?: string
    showA?: boolean
    showB?: boolean
    title?: string
  }>(),
  {
    labelA: '输出',
    labelB: '总',
    suffix: ' 速度',
    separator: '/',
    showA: true,
    showB: true,
    title: 'A/B tok/s: A 为纯吐字速度(扣首字)，B 为端到端总速度',
  }
)

const emit = defineEmits<{
  (e: 'update:showA', val: boolean): void
  (e: 'update:showB', val: boolean): void
  (e: 'change', val: { showA: boolean; showB: boolean }): void
}>()

function toggleA(e: MouseEvent) {
  e.stopPropagation()
  const next = !props.showA
  emit('update:showA', next)
  emit('change', { showA: next, showB: props.showB })
}

function toggleB(e: MouseEvent) {
  e.stopPropagation()
  const next = !props.showB
  emit('update:showB', next)
  emit('change', { showA: props.showA, showB: next })
}
</script>

<style scoped>
.dual-value-header {
  display: inline-flex;
  align-items: center;
  user-select: none;
  white-space: nowrap;
}

.dual-item {
  cursor: pointer;
  border-bottom: 1px dashed currentColor;
  padding-bottom: 1px;
  line-height: 1.1;
  transition: opacity 0.18s ease, filter 0.18s ease, border-color 0.18s ease;
}

.dual-item:hover {
  filter: brightness(1.25);
  border-bottom-style: solid;
}

.dual-item.is-dimmed {
  opacity: 0.32;
  border-bottom-color: transparent;
  filter: grayscale(1);
}

.dual-item.is-dimmed:hover {
  opacity: 0.65;
  border-bottom-color: currentColor;
  border-bottom-style: dashed;
}

.dual-separator {
  margin: 0 1px;
  transition: opacity 0.18s ease;
}

.dual-separator.is-dimmed {
  opacity: 0.25;
}

.dual-suffix {
  margin-left: 2px;
}
</style>
