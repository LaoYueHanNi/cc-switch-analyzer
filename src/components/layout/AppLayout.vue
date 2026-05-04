<template>
  <div class="app-layout">
    <!-- 工具栏始终可见 -->
    <div class="top-area">
      <Toolbar />
    </div>

    <!-- 加载遮罩层 -->
    <div v-if="dbStore.isLoading" class="overlay loading-overlay">
      <n-spin size="large" />
      <p class="overlay-text">正在加载数据库...</p>
    </div>

    <!-- 无数据库提示（带选择按钮 + 错误信息） -->
    <div v-else-if="!dbStore.isLoaded" class="overlay no-db-overlay">
      <n-icon size="48" color="#ccc"><server-outline /></n-icon>
      <p class="overlay-text">请选择 CC-Switch 数据库文件</p>
      <n-button type="primary" size="medium" @click="onSelectDb">
        <template #icon>
          <n-icon><folder-open-outline /></n-icon>
        </template>
        选择数据库
      </n-button>
      <p v-if="dbStore.error" class="error-text">错误：{{ dbStore.error }}</p>
    </div>

    <!-- 主界面 -->
    <template v-else>
      <!-- 筛选 + 摘要（仅模型/供应商 Tab） -->
      <div class="top-area">
        <template v-if="activeTab === 'by-model' || activeTab === 'by-provider'">
          <FilterBar />
          <SummaryBar />
        </template>
      </div>

      <!-- Tab 栏 -->
      <n-tabs
        :value="activeTab"
        @update:value="onTabChange"
        type="line"
        size="medium"
        class="main-tabs"
      >
        <n-tab-pane name="by-model" tab="按模型" />
        <n-tab-pane name="by-provider" tab="按供应商" />
        <n-tab-pane name="session" tab="会话分析" />
        <n-tab-pane name="realtime" tab="实时 Token" />
        <n-tab-pane name="pricing" tab="定价计算" />
      </n-tabs>

      <!-- 内容区 -->
      <div class="content-area">
        <router-view />
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { NSpin, NTabs, NTabPane, NButton, NIcon } from 'naive-ui'
import { FolderOpenOutline, ServerOutline } from '@vicons/ionicons5'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'
import Toolbar from './Toolbar.vue'
import FilterBar from './FilterBar.vue'
import SummaryBar from './SummaryBar.vue'

const router = useRouter()
const route = useRoute()
const dbStore = useDatabaseStore()
const { selectDatabase, autoLoadDatabase } = useDatabase()

// 当前活跃的Tab名称
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

// 启动时自动尝试加载默认数据库
onMounted(async () => {
  await autoLoadDatabase()
})
</script>

<style scoped>
.app-layout {
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* 工具栏区域 */
.top-area {
  flex-shrink: 0;
  background: #fff;
  border-bottom: 1px solid #e8e8e8;
}

/* 遮罩层 */
.overlay {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 16px;
}

.loading-overlay {
  background: rgba(255, 255, 255, 0.9);
}

.no-db-overlay {
  background: #f5f5f5;
}

.overlay-text {
  font-size: 16px;
  color: #666;
}

.error-text {
  font-size: 13px;
  color: #e74c3c;
  max-width: 500px;
  text-align: center;
  word-break: break-all;
}

/* Tab 栏 */
.main-tabs {
  flex-shrink: 0;
  padding: 0 12px;
  background: #fff;
  border-bottom: 1px solid #f0f0f0;
}

/* 内容区域 */
.content-area {
  flex: 1;
  overflow: auto;
  background: #f5f5f5;
  padding: 12px 16px;
}
</style>
