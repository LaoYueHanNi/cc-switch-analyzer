<template>
  <input
    type="text"
    class="compact-input"
    :value="modelValue"
    :placeholder="placeholder"
    :disabled="disabled"
    :maxlength="maxlength"
    @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    @keyup.enter="emit('enter')"
  />
</template>

<script setup lang="ts">
withDefaults(defineProps<{
  modelValue: string
  placeholder?: string
  disabled?: boolean
  maxlength?: number
}>(), {
  placeholder: '',
  disabled: false,
})

const emit = defineEmits<{
  'update:modelValue': [value: string]
  'enter': []
}>()
</script>

<style scoped>
.compact-input {
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
}
.compact-input:hover { border-color: var(--color-blue); }
.compact-input:focus { border-color: var(--color-blue); }
.compact-input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
  background: var(--bg-hover);
}
.compact-input::placeholder { color: var(--text-faint); }
</style>
