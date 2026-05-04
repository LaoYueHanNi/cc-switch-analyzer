<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN" :theme-overrides="themeOverrides">
    <AppLayout />
  </n-config-provider>
</template>

<script setup lang="ts">
import { zhCN, dateZhCN, NConfigProvider } from 'naive-ui'
import AppLayout from '@/components/layout/AppLayout.vue'

// Naive UI 主题微调
const themeOverrides = {
  common: {
    primaryColor: '#4a90d9',
    primaryColorHover: '#5a9fe9'
  }
}
</script>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  background-color: #f5f5f5;
  overflow: hidden;
}
</style>
