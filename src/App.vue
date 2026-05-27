<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN" :theme="themeStore.isDark ? darkTheme : undefined" :theme-overrides="themeOverrides">
    <n-message-provider>
      <AppLayout />
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { zhCN, dateZhCN, NConfigProvider, NMessageProvider, darkTheme } from 'naive-ui'
import AppLayout from '@/components/layout/AppLayout.vue'
import { useThemeStore } from '@/stores/theme'

const themeStore = useThemeStore()

onMounted(() => {
  themeStore.init()
  // 全局禁用浏览器右键菜单（Tauri webview）
  document.addEventListener('contextmenu', e => e.preventDefault())
})

const themeOverrides = computed(() => ({
  common: {
    primaryColor: '#4a90d9',
    primaryColorHover: '#5a9fe9',
    fontWeight: '500',
    fontWeightStrong: '600'
  }
}))
</script>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

:root {
  --bg-base: #f5f5f5;
  --bg-card: #fff;
  --bg-card-alt: #fafafa;
  --bg-hover: #f0f0f0;
  --bg-flash: #e8f5e9;

  --text-primary: #333;
  --text-secondary: #555;
  --text-tertiary: #666;
  --text-muted: #999;
  --text-faint: #bbb;

  --border-main: #e8e8e8;
  --border-light: #f0f0f0;
  --border-faint: #f2f2f2;

  --color-cost: #e74c3c;
  --color-green: #16a085;
  --color-amber: #e67e22;
  --color-amber-bg: #fef9e7;
  --color-teal: #e91e63;
  --color-teal-bg: #fce4ec;
  --color-blue-bg: #eef5ff;
  --color-purple: #8e44ad;
  --color-purple-bg: #f3e5f5;
  --color-orange: #f39c12;
  --color-blue: #2980b9;
  --color-indigo: #3f51b5;
  --color-dark-orange: #d35400;

  --shadow-card: 0 2px 6px rgba(0,0,0,0.06);

  /* 设计 token */
  --card-padding: 10px;
  --card-gap: 10px;
  --font-size-cost: 17px;
  --transition-speed: 0.2s;
}

html.dark {
  --bg-base: #1a1a2e;
  --bg-card: #222240;
  --bg-card-alt: #2a2a4a;
  --bg-hover: #303050;
  --bg-flash: #1a3a2a;

  --text-primary: #e0e0e0;
  --text-secondary: #c0c0c0;
  --text-tertiary: #a0a0a0;
  --text-muted: #777;
  --text-faint: #555;

  --border-main: #3a3a5a;
  --border-light: #2e2e4e;
  --border-faint: #2a2a4a;

  --color-cost: #ff6b6b;
  --color-green: #2ed8a4;
  --color-amber: #ffc857;
  --color-amber-bg: #3a3520;
  --color-teal: #f06292;
  --color-teal-bg: #3a1a25;
  --color-blue-bg: #1a2a40;
  --color-purple: #b370cf;
  --color-purple-bg: #2a1a30;
  --color-orange: #ffc857;
  --color-blue: #5dade2;
  --color-indigo: #7986cb;
  --color-dark-orange: #e67e22;

  --shadow-card: 0 2px 6px rgba(0,0,0,0.3);
}

html, body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  background-color: var(--bg-base);
  overflow: hidden;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  text-rendering: optimizeLegibility;
}

body {
  zoom: 1.1;
}

/* 隐藏滚动条 */
::-webkit-scrollbar {
  width: 4px;
  height: 4px;
}
::-webkit-scrollbar-track {
  background: transparent;
}
::-webkit-scrollbar-thumb {
  background: var(--border-main);
  border-radius: 2px;
}
::-webkit-scrollbar-thumb:hover {
  background: var(--text-muted);
}

/* 全局缩小下拉菜单 */
body .n-base-selection-option__content {
  font-size: 11px;
}

/* 全局加载/空状态 */
.tab-loading,
.tab-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 0;
  color: var(--text-muted);
  gap: 12px;
}
</style>
