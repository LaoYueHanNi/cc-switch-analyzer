<template>
  <span class="dual-value-cell">
    <!-- 全关: 两者均被点暗 -->
    <template v-if="!showA && !showB">
      <span class="val-fallback">{{ fallback }}</span>
    </template>

    <!-- 全开: 正常展示双值 (默认) -->
    <template v-else-if="showA && showB">
      <template v-if="hasA || hasB">
        <span :class="classA">{{ hasA ? valA : '-' }}</span>
        <span :class="separatorClass">{{ separator }}</span>
        <span :class="classB">{{ hasB ? valB : '-' }}</span>
        <span :class="unitClass">{{ unit }}</span>
      </template>
      <template v-else>
        <span class="val-fallback">{{ fallback }}</span>
      </template>
    </template>

    <!-- 仅展示 A (B 被点暗) -->
    <template v-else-if="showA">
      <template v-if="hasA">
        <span :class="classA">{{ valA }}</span>
        <span :class="unitClass">{{ unit }}</span>
      </template>
      <template v-else>
        <span class="val-fallback">{{ fallback }}</span>
      </template>
    </template>

    <!-- 仅展示 B (A 被点暗) -->
    <template v-else-if="showB">
      <template v-if="hasB">
        <span :class="classB">{{ valB }}</span>
        <span :class="unitClass">{{ unit }}</span>
      </template>
      <template v-else>
        <span class="val-fallback">{{ fallback }}</span>
      </template>
    </template>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue'

defineOptions({ name: 'DualValueCell' })

const props = withDefaults(
  defineProps<{
    valA?: string | number | null
    valB?: string | number | null
    showA?: boolean
    showB?: boolean
    unit?: string
    separator?: string
    classA?: string
    classB?: string
    unitClass?: string
    separatorClass?: string
    fallback?: string
  }>(),
  {
    showA: true,
    showB: true,
    unit: ' tok/s',
    separator: '/',
    classA: 'spd-stream',
    classB: 'spd-total',
    unitClass: 'spd-unit',
    separatorClass: 'spd-slash',
    fallback: '-',
  }
)

const hasA = computed(() => {
  return props.valA !== undefined && props.valA !== null && props.valA !== '' && props.valA !== '-'
})

const hasB = computed(() => {
  return props.valB !== undefined && props.valB !== null && props.valB !== '' && props.valB !== '-'
})
</script>

<style scoped>
.dual-value-cell {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  white-space: nowrap;
}

.spd-stream {
  color: var(--color-teal);
  font-weight: 600;
}

.spd-slash {
  color: var(--text-muted);
  margin: 0 1px;
}

.spd-total {
  color: var(--color-orange);
}

.spd-unit {
  color: var(--text-muted);
  font-size: 10px;
}

.val-fallback {
  color: var(--text-muted);
}
</style>
