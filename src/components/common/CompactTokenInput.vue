<template>
  <span class="compact-token-input">
    <input
      type="number"
      class="token-number"
      :value="displayValue"
      min="0"
      step="any"
      :aria-label="`${label} Token（${unit}）`"
      @input="onInput"
      @blur="onCommit"
      @keydown.enter.prevent="onCommit"
    />
    <span class="token-unit">{{ unit }}</span>
  </span>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useTokenInput } from '@/composables/useTokenInput'

const props = defineProps<{
  modelValue: number
  label: string
}>()
const emit = defineEmits<{ 'update:modelValue': [tokens: number] }>()
const { value, unit, tokens, setTokens, updateValue, commitValue } = useTokenInput(props.modelValue)
const displayValue = ref(value.value === null ? '' : String(value.value))

function syncDisplay(input?: HTMLInputElement): void {
  displayValue.value = value.value === null ? '' : String(value.value)
  if (input) input.value = displayValue.value
}

watch(() => props.modelValue, amount => {
  if (amount !== tokens.value) {
    setTokens(amount)
    syncDisplay()
  }
})

function onInput(event: Event): void {
  const input = event.target as HTMLInputElement
  const amount = input.value === '' ? null : input.valueAsNumber
  // 保留正在编辑的原始文本，费用照常按当前单位更新。
  displayValue.value = input.value
  updateValue(amount)
  if (amount !== null && amount < 0) syncDisplay(input)
  emit('update:modelValue', tokens.value)
}

function onCommit(event: Event): void {
  commitValue()
  syncDisplay(event.target as HTMLInputElement)
}
</script>

<style scoped>
.compact-token-input {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.token-number {
  width: 72px;
  height: 20px;
  padding: 1px 4px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: var(--bg-card);
  color: var(--text-primary);
  font-size: 11px;
  line-height: 20px;
  outline: none;
  transition: border-color 0.15s;
  -moz-appearance: textfield;
}
.token-number::-webkit-inner-spin-button,
.token-number::-webkit-outer-spin-button {
  -webkit-appearance: none;
  margin: 0;
}
.token-number:hover,
.token-number:focus { border-color: var(--color-blue); }
.token-unit {
  min-width: 11px;
  color: var(--text-tertiary);
  font-size: 11px;
}
</style>
