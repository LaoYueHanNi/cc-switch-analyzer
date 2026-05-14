<template>
  <div class="session-analysis">
    <!-- 排序选择器 -->
    <div class="session-toolbar">
      <n-select
        v-model:value="sortBy"
        :options="sortOptions"
        size="tiny"
        style="width: 120px"
        placeholder="排序方式"
        teleport-disabled
      />
    </div>

    <div v-if="loading" class="tab-loading">
      <n-spin size="medium" />
      <p>正在查询会话数据...</p>
    </div>

    <div v-else-if="sessionCards.length === 0" class="tab-empty">
      <p>{{ dbStore.hasDatabase ? '暂无会话数据' : '请先选择数据库文件' }}</p>
    </div>

    <div v-else class="session-list">
      <SessionCard
        v-for="s in sessionCards"
        :key="s.sessionId"
        v-bind="s"
        :title="getTitle(s.sessionId)"
        :project="getProject(s.sessionId)"
        :title-source="getSource(s.sessionId)"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
defineOptions({ name: 'SessionAnalysis' })
import { ref, computed, watch, onActivated, onDeactivated } from 'vue'
import { NSelect, NSpin } from 'naive-ui'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useSessionTitles } from '@/composables/useSessionTitles'
import SessionCard from '@/components/session/SessionCard.vue'
import type { SessionModelCostEntry } from '@/types/common'

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const { getTitle, getProject, getSource, fetchTitles } = useSessionTitles()
const loading = ref(false)
const sortBy = ref('totalCost')
const isActive = ref(true)
const needsRefresh = ref(false)

const sortOptions = [
  { label: '费用', value: 'totalCost' },
  { label: 'Token 数量', value: 'totalTokens' },
  { label: '请求数', value: 'requestCount' },
  { label: '最大上下文', value: 'maxContextWidth' },
  { label: '缓存命中率', value: 'cacheHitRate' }
]

interface SessionCardData {
  sessionId: string
  totalCost: number
  totalTokens: number
  requestCount: number
  durationSec: number
  startTime: number
  endTime: number
  maxContextWidth: number
  cacheHitRate: number
  timestamps: number[]
  modelBreakdown: SessionModelCostEntry[]
  sources: string[]
}

const sessionsRaw = ref<SessionCardData[]>([])

const sessionCards = computed<SessionCardData[]>(() => {
  const list = sessionsRaw.value.map(s => {
    // 每个会话内的模型固定按费用降序
    const sortedModels = [...s.modelBreakdown].sort((a, b) => (b.cost || 0) - (a.cost || 0))
    return { ...s, modelBreakdown: sortedModels }
  })

  // 对会话列表排序
  const sk = sortBy.value as keyof SessionCardData
  list.sort((a, b) => (b[sk] as number) - (a[sk] as number))
  return list
})

async function loadData(): Promise<void> {
  if (!dbStore.hasDatabase) return
  loading.value = true
  try {
    const params = filterStore.filterParams
    console.log('[SessionAnalysis] 查询会话')
    const result = await platformAdapter.querySessionsWithCost(params)
    console.log('[SessionAnalysis] 会话结果数:', result?.length || 0)
    sessionsRaw.value = result || []
    if (result && result.length > 0) {
      fetchTitles(result.map(s => s.sessionId).filter(Boolean))
    }
  } catch (err: any) {
    console.error('[SessionAnalysis] 会话查询失败:', err.message || err)
    sessionsRaw.value = []
  } finally {
    loading.value = false
  }
}

function tryLoadData(): void {
  if (isActive.value) {
    loadData()
  } else {
    needsRefresh.value = true
  }
}

let sessionFilterTimer: ReturnType<typeof setTimeout> | null = null

watch(() => dbStore.hasDatabase, (val) => { if (val) tryLoadData() }, { immediate: true })
watch(() => filterStore.filterParams, () => {
  if (sessionFilterTimer) clearTimeout(sessionFilterTimer)
  sessionFilterTimer = setTimeout(() => tryLoadData(), 300)
}, { deep: true })
watch(() => dbStore.refreshVersion, () => { if (dbStore.hasDatabase) tryLoadData() })

onActivated(() => {
  isActive.value = true
  if (needsRefresh.value) {
    needsRefresh.value = false
    loadData()
  }
})

onDeactivated(() => {
  isActive.value = false
})
</script>

<style scoped>
.session-analysis {
  min-height: 200px;
  display: flex;
  flex-direction: column;
}

.session-toolbar {
  padding: 2px 0 6px;
}

.tab-loading,
.tab-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 0;
  color: var(--text-muted);
  gap: 12px;
  flex: 1;
}

.session-list {
  flex: 1;
  overflow-y: auto;
}
</style>
