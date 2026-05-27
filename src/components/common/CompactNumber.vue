<template>
  <input
    type="number"
    class="compact-number"
    :value="modelValue"
    :min="min"
    :max="max"
    :step="step"
    :disabled="disabled"
    :style="{ width }"
    @input="emit('update:modelValue', parseNumber(($event.target as HTMLInputElement).value))"
  />
</template>

<script setup lang="ts">
withDefaults(defineProps<{
  modelValue: number | null
  min?: number
  max?: number
  step?: number
  disabled?: boolean
  width?: string
}>(), {
  min: undefined,
  max: undefined,
  step: 1,
  disabled: false,
  width: '130px',
})

const emit = defineEmits<{ 'update:modelValue': [value: number | null] }>()

function parseNumber(val: string): number | null {
  if (val === '') return null
  const n = Number(val)
  return isNaN(n) ? null : n
}
</script>

<style scoped>
.compact-number {
  font-size: 11px;
  color: var(--text-primary);
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 3px;
  padding: 1px 4px;
  height: 20px;
  line-height: 20px;
  outline: none;
  transition: border-color 0.15s;
  -moz-appearance: textfield;
}
.compact-number::-webkit-inner-spin-button,
.compact-number::-webkit-outer-spin-button {
  -webkit-appearance: none; margin: 0;
}
.compact-number:hover { border-color: var(--color-blue); }
.compact-number:focus { border-color: var(--color-blue); }
.compact-number:disabled {
  opacity: 0.5;
  cursor: not-allowed;
  background: var(--bg-hover);
}
</style>
