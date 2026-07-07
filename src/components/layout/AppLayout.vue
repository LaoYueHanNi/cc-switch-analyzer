<template>
  <div class="app-layout" ref="appLayoutRef">
    <!-- 工具栏始终可见 -->
    <div class="top-area">
      <Toolbar />
    </div>

    <!-- 加载遮罩层 -->
    <div v-if="dbStore.isLoading" class="overlay loading-overlay">
      <n-spin size="large" />
      <p class="overlay-text">正在加载数据库...</p>
    </div>

    <!-- 无数据库提示 -->
    <div v-else-if="!dbStore.isLoaded" class="overlay no-db-overlay">
      <n-icon size="48" class="no-db-icon"><server-outline /></n-icon>
      <p class="overlay-text">请选择 CC-Switch 数据库文件</p>
      <n-button type="primary" size="medium" @click="onSelectDb">
        <template #icon>
          <n-icon><folder-open-outline /></n-icon>
        </template>
        选择数据库
      </n-button>
      <p v-if="dbStore.error" class="error-text">错误：{{ dbStore.error }}</p>
    </div>

    <!-- 主界面：侧边栏 + 内容 -->
    <div v-else class="main-body">
      <!-- 左侧导航 -->
      <div class="sidebar">
        <div
          v-for="item in navItems"
          :key="item.name"
          class="sidebar-item"
          :class="{ active: activeTab === item.name }"
          @click="onTabChange(item.name)"
        >
          <n-icon size="18"><component :is="item.icon" /></n-icon>
          <span class="sidebar-label">{{ item.label }}</span>
        </div>

        <div class="sidebar-spacer" />

        <!-- 刷新间隔：点击循环切换 -->
        <div class="sidebar-item sidebar-bottom" @click="onCycleInterval" :title="intervalTitle">
          <n-icon size="16"><refresh-outline /></n-icon>
          <span class="sidebar-label interval-label">{{ intervalDisplay }}</span>
        </div>

        <!-- 暗色模式切换 -->
        <div class="sidebar-item sidebar-bottom" :title="themeStore.isDark ? '切换亮色' : '切换暗色'" @click="themeStore.toggle()">
          <n-icon size="16"><sunny-outline v-if="themeStore.isDark" /><moon-outline v-else /></n-icon>
          <span class="sidebar-label">{{ themeStore.isDark ? '亮色' : '暗色' }}</span>
        </div>
      </div>

      <!-- 右侧内容 -->
      <div class="main-content">
        <!-- 筛选 + 摘要（仅模型/供应商 Tab） -->
        <template v-if="activeTab === 'by-model' || activeTab === 'by-provider' || activeTab === 'trend'">
          <FilterBar />
          <SummaryBar />
        </template>

        <!-- 内容区 -->
        <div class="content-area">
          <router-view v-slot="{ Component }">
            <KeepAlive>
              <component :is="Component" />
            </KeepAlive>
          </router-view>
        </div>
      </div>
    </div>

    <UpdateNotification />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch, type Component } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { NSpin, NButton, NIcon } from 'naive-ui'
import {
  FolderOpenOutline, ServerOutline,
  GridOutline, BusinessOutline, TrendingUpOutline, ChatbubblesOutline,
  PulseOutline, CalculatorOutline, ClipboardOutline,
  MoonOutline, SunnyOutline, RefreshOutline
} from '@vicons/ionicons5'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useQueryStore } from '@/stores/query'
import { useThemeStore } from '@/stores/theme'
import { useDatabase } from '@/composables/useDatabase'
import { useAutoRefresh } from '@/composables/useAutoRefresh'
import { useUpdaterStore } from '@/stores/updater'
import { platformAdapter } from '@/platform'
import Toolbar from './Toolbar.vue'
import FilterBar from './FilterBar.vue'
import SummaryBar from './SummaryBar.vue'
import UpdateNotification from './UpdateNotification.vue'

const router = useRouter()
const route = useRoute()
const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const queryStore = useQueryStore()
const themeStore = useThemeStore()
const { selectDatabase, autoLoadDatabase, refreshDatabase } = useDatabase()
const updaterStore = useUpdaterStore()

const navItems: { name: string; label: string; icon: Component }[] = [
  { name: 'by-model', label: '模型', icon: GridOutline },
  { name: 'by-provider', label: '供应商', icon: BusinessOutline },
  { name: 'trend', label: '趋势', icon: TrendingUpOutline },
  { name: 'session', label: '会话', icon: ChatbubblesOutline },
  { name: 'realtime', label: '实时', icon: PulseOutline },
  { name: 'pricing', label: '定价', icon: CalculatorOutline },
  { name: 'task', label: '任务', icon: ClipboardOutline }
]

const activeTab = computed(() => {
  const name = route.name as string
  return name || 'by-model'
})

function onTabChange(tabName: string): void {
  router.push({ name: tabName })
}

async function onSelectDb(): Promise<void> {
  await selectDatabase()
}

// ===== 自动刷新 =====
const { intervalDisplay, intervalTitle, cycleInterval: onCycleInterval } = useAutoRefresh(() => {
  if (dbStore.hasDatabase) refreshDatabase()
})

// body { zoom: 1.1 } 导致 macOS WebKit 下 calc(100vh / 1.1) 精度不足
// 使用 window.innerHeight 精确补偿 zoom 倍率
const appLayoutRef = ref<HTMLElement>()
const syncLayoutHeight = () => {
  if (!appLayoutRef.value) return
  appLayoutRef.value.style.height = `${window.innerHeight / 1.1}px`
}

// 集中查询触发：筛选变化（防抖 300ms）、数据库加载、刷新时统一执行一次 queryPrecompute
let filterTimer: ReturnType<typeof setTimeout> | null = null
watch(() => filterStore.filterParams, () => {
  if (filterTimer) clearTimeout(filterTimer)
  filterTimer = setTimeout(() => {
    if (dbStore.hasDatabase) queryStore.executeQuery(filterStore.filterParams)
  }, 300)
}, { deep: true })
watch(() => dbStore.hasDatabase, (val) => {
  if (val) queryStore.executeQuery(filterStore.filterParams)
}, { immediate: true })
watch(() => dbStore.refreshVersion, () => {
  if (dbStore.hasDatabase) queryStore.executeQuery(filterStore.filterParams, true)
})

let unlistenCheckUpdate: (() => void) | null = null

onMounted(async () => {
  syncLayoutHeight()
  window.addEventListener('resize', syncLayoutHeight)
  await autoLoadDatabase()
  unlistenCheckUpdate = await platformAdapter.onCheckUpdateRequested(() => updaterStore.checkForUpdate())
})

onUnmounted(() => {
  window.removeEventListener('resize', syncLayoutHeight)
  unlistenCheckUpdate?.()
  unlistenCheckUpdate = null
})
</script>

<style scoped>
.app-layout {
  /* height 由 JS syncLayoutHeight 动态设置，避免 CSS 100vh + zoom 精度问题 */
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.top-area {
  flex-shrink: 0;
  background: var(--bg-card);
  border-bottom: 1px solid var(--border-main);
}

.overlay {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 16px;
}

.loading-overlay {
  background: var(--bg-card);
}

.no-db-overlay {
  background: var(--bg-base);
}

.overlay-text {
  font-size: 16px;
  color: var(--text-tertiary);
}

.error-text {
  font-size: 13px;
  color: var(--color-cost);
  max-width: 500px;
  text-align: center;
  word-break: break-all;
}

.no-db-icon {
  color: var(--text-faint);
}

/* 主区域 */
.main-body {
  flex: 1;
  display: flex;
  min-height: 0;
}

/* 左侧导航 */
.sidebar {
  width: 64px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  background: var(--bg-card);
  border-right: 1px solid var(--border-main);
  padding: 6px 0;
}

.sidebar-item {
  width: 56px;
  padding: 6px 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  border-radius: 6px;
  cursor: pointer;
  color: var(--text-muted);
  transition: background var(--transition-speed), color var(--transition-speed);
  position: relative;
}

.sidebar-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.sidebar-item.active {
  background: var(--bg-hover);
  color: var(--color-blue);
}

.sidebar-item.active::before {
  content: '';
  position: absolute;
  left: -4px;
  top: 6px;
  bottom: 6px;
  width: 3px;
  border-radius: 2px;
  background: var(--color-blue);
}

.sidebar-label {
  font-size: 9px;
  line-height: 1;
  white-space: nowrap;
}

.interval-label {
  color: var(--color-amber);
}

.sidebar-spacer {
  flex: 1;
}

.sidebar-bottom {
  padding: 4px 0;
}

/* 右侧内容 */
.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.content-area {
  flex: 1;
  overflow: auto;
  background: var(--bg-base);
  padding: 12px 16px;
}
</style>
