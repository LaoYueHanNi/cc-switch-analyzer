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
  background: #fff;
  border-radius: 4px;
  border: 1px solid #e8e8e8;
  border-left-width: 3px;
}

.stat-label {
  font-size: 11px;
  color: #999;
  white-space: nowrap;
}

.stat-value {
  font-size: 14px;
  font-weight: 600;
  margin-top: 2px;
}

.stat-subtitle {
  font-size: 10px;
  color: #bbb;
  margin-top: 1px;
}
</style>
