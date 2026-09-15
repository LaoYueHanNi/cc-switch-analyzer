<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN" :theme="themeStore.isDark ? darkTheme : undefined" :theme-overrides="themeOverrides">
    <n-message-provider :container-style="{ zIndex: '20000' }">
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

const themeOverrides = computed(() => {
  const dark = themeStore.isDark
  const primary = dark ? '#8096d0' : '#4d7cc4'
  const primaryHover = dark ? '#96aad9' : '#5e8bd0'
  return {
    common: {
      primaryColor: primary,
      primaryColorHover: primaryHover,
      primaryColorPressed: primary,
      fontWeight: '500',
      fontWeightStrong: '600'
    }
  }
})
</script>

<style>
@font-face {
  font-family: 'Inter';
  src: url('@/assets/fonts/InterVariable.woff2') format('woff2');
  font-weight: 100 900;
  font-display: swap;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

:root {
  --bg-base: #f1f3f5;
  --bg-card: #ffffff;
  --bg-card-alt: #f4f5f7;
  --bg-hover: #e9ecf2;
  --bg-sidebar: #f9fafb;
  --bg-flash: #d8e8d8;

  --text-primary: #0f1115;
  --text-secondary: #61656b;
  --text-tertiary: #81858c;
  --text-muted: #979da6;
  --text-faint: #adb2b8;

  --border-main: rgba(0, 0, 0, 0.10);
  --border-light: rgba(0, 0, 0, 0.06);
  --border-faint: rgba(0, 0, 0, 0.04);

  --color-cost: #ec1313;
  --color-green: #22c55e;
  --color-amber: #f59e0b;
  --color-amber-bg: #fef9e7;
  --color-teal: #4d7cc4;
  --color-teal-bg: #eef2ff;
  --color-blue-bg: #eef2ff;
  --color-purple: #7a6498;
  --color-purple-bg: #f4f0f7;
  --color-orange: #dd8629;
  --color-blue: #4d7cc4;
  --color-indigo: #5b6b82;
  --color-dark-orange: #f59e0b;

  --shadow-card: 0 0 0 0.5px rgba(0, 0, 0, 0.16), 0 3px 8px rgba(0, 0, 0, 0.03);
  --shadow-focus: 0 0 0 0.5px rgba(0, 0, 0, 0.16), 0 8px 24px rgba(0, 0, 0, 0.08);

  --card-padding: 10px;
  --card-gap: 12px;
  --card-radius: 12px;
  --chip-radius: 999px;
  --font-size-cost: 20px;
  --transition-speed: 0.16s;
}

html.dark {
  --bg-base: #151517;
  --bg-card: #232324;
  --bg-card-alt: #1c1c1e;
  --bg-hover: #353638;
  --bg-sidebar: #1b1b1c;
  --bg-flash: #1a3a2a;

  --text-primary: #f9fafb;
  --text-secondary: #cfd3d6;
  --text-tertiary: #adb2b8;
  --text-muted: #81858c;
  --text-faint: #61656b;

  --border-main: rgba(255, 255, 255, 0.12);
  --border-light: rgba(255, 255, 255, 0.08);
  --border-faint: rgba(255, 255, 255, 0.06);

  --color-cost: #f25a5a;
  --color-green: #4ed17e;
  --color-amber: #f7ad31;
  --color-amber-bg: #3a3520;
  --color-teal: #8096d0;
  --color-teal-bg: #1a2a40;
  --color-blue-bg: #1a2a40;
  --color-purple: #cbb8dc;
  --color-purple-bg: #2a2430;
  --color-orange: #f7ad31;
  --color-blue: #8096d0;
  --color-indigo: #b4becc;
  --color-dark-orange: #dd8629;

  --shadow-card: 0 0 0 0.5px rgba(255, 255, 255, 0.10), 0 3px 8px rgba(0, 0, 0, 0.25);
  --shadow-focus: 0 0 0 0.5px rgba(255, 255, 255, 0.18), 0 8px 24px rgba(0, 0, 0, 0.35);
}

html, body {
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  background-color: var(--bg-base);
  overflow: hidden;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  text-rendering: optimizeLegibility;
}

/* Naive UI Modal/Card 背景色跟随全局主题 */
.n-card {
  background-color: var(--bg-card) !important;
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
