<template>
  <div class="stat-card" :style="{ borderLeftColor: color }">
    <div class="stat-label">{{ label }}</div>
    <div class="stat-value" :style="{ color }">{{ value }}</div>
    <div v-if="subtitle" class="stat-subtitle">{{ subtitle }}</div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  label: string
  value: string
  color: string
  subtitle?: string
}>()
</script>

<style scoped>
.stat-card {
  flex-shrink: 0;
  min-width: 100px;
  padding: 6px 10px;
  background: var(--bg-card);
  border-radius: 4px;
  border: 1px solid var(--border-main);
  border-left-width: 3px;
}

.stat-label {
  font-size: 11px;
  color: var(--text-muted);
  white-space: nowrap;
}

.stat-value {
  font-size: 14px;
  font-weight: 600;
  margin-top: 2px;
}

.stat-subtitle {
  font-size: 10px;
  color: var(--text-faint);
  margin-top: 1px;
}
</style>
