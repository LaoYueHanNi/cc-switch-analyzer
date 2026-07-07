<template>
  <div class="task-detail">
    <template v-if="store.currentDetail">
      <div class="detail-header">
        <button class="back-btn" @click="onBack">&larr; 返回任务列表</button>
        <span class="detail-title">{{ store.currentDetail.title }}</span>
        <span class="status-tag" :class="`status-tag--${store.currentDetail.status}`">
          {{ statusInfo?.label || store.currentDetail.status }}
        </span>
      </div>

      <div v-if="store.currentDetail.description" class="description">
        {{ store.currentDetail.description }}
      </div>

      <div class="summary-bar">
        <div class="summary-item">
          <span class="value">{{ store.currentDetail.sessionCount }}</span>
          <span class="label">个会话</span>
        </div>
        <div class="summary-item">
          <span class="value">{{ formatNum(store.currentDetail.totalTokens) }}</span>
          <span class="label">Token</span>
        </div>
        <div class="summary-item">
          <span class="value cost">{{ formatCost(store.currentDetail.totalCost) }}</span>
          <span class="label">总费用</span>
        </div>
        <div class="summary-item muted">
          <span class="label">创建于 {{ formatTime(store.currentDetail.createdAt) }}</span>
        </div>
        <div class="summary-spacer" />
        <button class="cd-btn primary" @click="onAddSessions">添加会话</button>
      </div>

      <!-- 任务下确实没有绑定会话:用 currentDetail(已知立即可得)判断,避免和"加载中"空态闪烁 -->
      <div
        v-if="sessionDetails.length === 0 && store.currentDetail && store.currentDetail.sessions.length === 0"
        class="tab-empty small"
      >
        <p>该任务下还没有绑定会话,点击右上角"添加会话"。</p>
      </div>

      <div v-else-if="sessionDetails.length > 0" class="session-list">
        <div
          v-for="s in sessionDetails"
          :key="s.sessionId"
          class="session-wrap"
        >
          <SessionCard
            :session-id="s.sessionId"
            :title="s.title || sessionFromList(s.sessionId)?.title"
            :project="basenameDir(s.projectDir || sessionFromList(s.sessionId)?.projectDir)"
            :total-cost="s.totalCost"
            :total-tokens="s.totalTokens"
            :request-count="s.requestCount"
            :duration-sec="s.durationSec"
            :start-time="s.startTime"
            :end-time="s.endTime"
            :max-context-width="s.maxContextWidth"
            :cache-hit-rate="s.cacheHitRate"
            :timestamps="s.timestamps"
            :model-breakdown="s.modelBreakdown"
          />
          <div class="wrap-actions">
            <span
              v-if="s.sourceType === 'codex'"
              class="action-terminal action-codex"
              @click="onResumeCodex(s.sessionId, s.projectDir)"
              title="恢复 Codex 会话"
            ><span v-html="codexSvg"></span></span>
            <span
              v-else-if="s.sourceType === 'opencode'"
              class="action-terminal action-opencode"
              @click="onResumeOpenCode(s.sessionId, s.projectDir)"
              title="恢复 OpenCode 会话"
            ><span v-html="opencodeSvg"></span></span>
            <span
              v-else
              class="action-terminal"
              @click="onResumeClaude(s.sessionId, s.projectDir)"
              @contextmenu.prevent="onContextResumeClaude(s.sessionId, s.projectDir, $event)"
              title="恢复 Claude 会话（右键选择供应商配置）"
            ><span v-html="claudeSvg"></span></span>
          </div>
        </div>
      </div>
    </template>

    <SessionPickerDialog
      v-model:show="pickerShow"
      :submitting="pickerSaving"
      @submit="onPickerSubmit"
    />

    <!-- Claude Code 供应商右键菜单(与 SessionAnalysis 保持一致) -->
    <ProviderContextMenu
      :menu="providerMenu.menu"
      :adjust-position="providerMenu.adjustMenuPosition"
      @select="providerMenu.selectItem"
      @close="providerMenu.closeMenu"
    />

    <!-- 仅当 loadDetail 确认失败时显示(不会和"加载中"瞬态冲突) -->
    <div v-if="loadFailed" class="tab-empty">
      <p>未找到该任务,可能已被删除。</p>
      <button class="cd-btn" @click="onBack">返回</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useMessage } from 'naive-ui'
import { useTaskStore } from '@/stores/task'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { platformAdapter } from '@/platform'
import { useProviderContextMenu } from '@/composables/useProviderContextMenu'
import { useSessionResume } from '@/composables/useSessionResume'
import { formatNum, formatCost, epochToDateTimeStr } from '@/utils/format'
import { TASK_STATUS_OPTIONS, type TaskSessionInput } from '@/types/task'
import type { ProjectSessionDetail } from '@/platform/types'
import SessionCard from '@/components/session/SessionCard.vue'
import SessionPickerDialog from '@/components/task/SessionPickerDialog.vue'
import ProviderContextMenu from '@/components/common/ProviderContextMenu.vue'
import claudeSvg from '@/assets/claude.svg?raw'
import opencodeSvg from '@/assets/opencode.svg?raw'
import codexSvg from '@/assets/codex.svg?raw'

defineOptions({ name: 'TaskDetail' })

const route = useRoute()
const router = useRouter()
const store = useTaskStore()
const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const message = useMessage()

const sessionDetails = ref<ProjectSessionDetail[]>([])
const loadFailed = ref(false)
const pickerShow = ref(false)
const pickerSaving = ref(false)

// Claude Code 供应商右键菜单(与会话 tab 一致)
const providerMenu = useProviderContextMenu('TaskDetail')
let pendingResumeSessionId = ''
let pendingResumeProjectDir = ''

const ccswitchDbPath = computed(() =>
  dbStore.sources.find(s => s.dbType === 'CC-Switch')?.path
)

const taskId = computed(() => Number(route.params.id))

const statusInfo = computed(() => {
  const s = store.currentDetail?.status
  return TASK_STATUS_OPTIONS.find(o => o.value === s)
})

function sessionFromList(sessionId: string) {
  return store.currentDetail?.sessions.find(s => s.sessionId === sessionId)
}

function basenameDir(p?: string): string {
  if (!p) return ''
  // 与后端 std::path::Path::file_name() 行为一致:取最后一段(同时处理 / 和 \)
  const trimmed = p.replace(/[\\/]+$/, '')
  const i = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  return i >= 0 ? trimmed.slice(i + 1) : trimmed
}

function formatTime(ts: number): string {
  if (!ts) return '-'
  return epochToDateTimeStr(ts)
}

function onBack() {
  router.push({ name: 'task' })
}

async function loadDetail(id: number) {
  // 切任务时同时清空 store 和本地数据,避免旧任务的"无会话"提示在切换瞬间闪一下
  sessionDetails.value = []
  loadFailed.value = false
  store.currentDetail = null
  await store.fetchDetail(id)
  if (!store.currentDetail) {
    // 仅当 fetchDetail 真的报错(非"还在加载")时显示"未找到"
    loadFailed.value = !!store.error
    return
  }
  await loadSessionDetails()
}

async function loadSessionDetails() {
  const sessions = store.currentDetail?.sessions || []
  if (sessions.length === 0) {
    sessionDetails.value = []
    return
  }

  // 一次性批量拉详情(与会话 tab 二级页一致);不在中途渲染 stub/spinner,避免闪烁
  const ids = sessions.map(s => s.sessionId)
  try {
    const details = await platformAdapter.queryProjectSessionDetails(
      filterStore.filterParams, ids
    )
    // 若期间切换了任务,丢弃过期结果
    if (store.currentDetail && taskId.value === Number(route.params.id)) {
      sessionDetails.value = details
    }
  } catch (err: any) {
    console.error('[TaskDetail] 批量加载会话详情失败:', err?.message || err)
  }
}

onMounted(() => {
  loadDetail(taskId.value)
})

watch(() => route.params.id, (id) => {
  if (id) loadDetail(Number(id))
})

function onAddSessions() {
  pickerShow.value = true
}

async function onPickerSubmit(sessions: TaskSessionInput[]) {
  pickerSaving.value = true
  try {
    await store.addSessions(taskId.value, sessions)
    message.success(`已添加 ${sessions.length} 个会话`)
    pickerShow.value = false
    await loadSessionDetails()
  } catch (e: any) {
    message.error(String(e?.message || e))
  } finally {
    pickerSaving.value = false
  }
}


// ===== 会话恢复(与 SessionAnalysis 二级页保持一致) =====

const { resumeSession } = useSessionResume('TaskDetail', (label, e) => {
  message.error(`恢复 ${label} 会话失败: ` + (e?.message || e))
})

async function onResumeClaude(sessionId: string, projectDir?: string) {
  await resumeSession('claude', sessionId, projectDir)
}

async function onResumeOpenCode(sessionId: string, projectDir?: string) {
  await resumeSession('opencode', sessionId, projectDir)
}

async function onResumeCodex(sessionId: string, projectDir?: string) {
  await resumeSession('codex', sessionId, projectDir)
}

async function onContextResumeClaude(sessionId: string, projectDir: string | undefined, event: MouseEvent) {
  const dbPath = ccswitchDbPath.value
  if (!dbPath) return
  const items = await providerMenu.loadProviderItems(dbPath)
  if (items.length === 0) {
    message.info('暂无可用供应商配置')
    return
  }
  pendingResumeSessionId = sessionId
  pendingResumeProjectDir = projectDir || ''
  providerMenu.openMenu(event, items, (providerId) => onProviderItemSelect(providerId))
}

async function onProviderItemSelect(providerId: string) {
  try {
    await platformAdapter.resumeClaudeSessionWithProvider(
      pendingResumeSessionId,
      providerId,
      ccswitchDbPath.value!,
      pendingResumeProjectDir || undefined
    )
  } catch (e: any) {
    message.error('恢复失败: ' + (e?.message || e))
  }
}
</script>

<style scoped>
.task-detail {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 100%;
}

.tab-loading, .tab-empty {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  padding: 60px 0; gap: 12px; color: var(--text-muted);
}
.tab-loading.small {
  padding: 30px 0;
}
.tab-empty.small {
  padding: 30px 0;
}

.detail-header {
  display: flex;
  align-items: center;
  gap: 8px;
}
.back-btn {
  font-size: 12px;
  color: var(--color-blue);
  background: none;
  border: none;
  cursor: pointer;
  padding: 2px 8px;
  border-radius: 3px;
  transition: background 0.15s;
}
.back-btn:hover { background: var(--bg-hover); }
.detail-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.status-tag {
  font-size: 10px;
  padding: 0 6px;
  height: 16px;
  line-height: 16px;
  border-radius: 2px;
  background: var(--bg-hover);
  color: var(--text-muted);
  border: 1px solid var(--border-main);
}
.status-tag--todo { color: var(--text-muted); }
.status-tag--in_progress { color: var(--color-blue); border-color: var(--color-blue); }
.status-tag--done { color: var(--color-green); border-color: var(--color-green); }
.status-tag--archived { color: var(--text-faint); }

.description {
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-card);
  border: 1px solid var(--border-light);
  border-radius: 4px;
  padding: 8px 10px;
  white-space: pre-wrap;
  word-break: break-word;
}

.summary-bar {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 8px 12px;
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 6px;
}
.summary-item {
  display: flex;
  align-items: baseline;
  gap: 4px;
}
.summary-item .value {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}
.summary-item .value.cost {
  color: var(--color-cost);
  font-size: var(--font-size-cost);
}
.summary-item .label {
  font-size: 10px;
  color: var(--text-muted);
}
.summary-item.muted .label {
  color: var(--text-tertiary);
}
.summary-spacer {
  flex: 1;
}

.session-list {
  display: flex;
  flex-direction: column;
}
.session-wrap {
  position: relative;
  margin-bottom: 10px;
}
.wrap-actions {
  position: absolute; top: 8px; right: 8px;
  display: flex; gap: 14px; z-index: 1;
}
.session-wrap:hover .wrap-actions { opacity: 1; }
.action-terminal {
  display: inline-flex; align-items: center; justify-content: center;
  cursor: pointer; font-size: 11px;
  color: var(--text-tertiary);
  transition: color 0.15s;
}
.action-terminal :deep(svg) {
  width: 14px; height: 14px;
}
.action-terminal:hover {
  color: var(--color-blue);
}
.session-skeleton {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 14px;
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 8px;
  margin-bottom: 10px;
  color: var(--text-muted);
  font-size: 12px;
}
.skeleton-meta {
  font-family: monospace;
}

.spinner {
  display: inline-block;
  width: 14px; height: 14px;
  border: 2px solid var(--border-main);
  border-top-color: var(--color-blue);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  flex-shrink: 0;
}
.spinner-lg { width: 24px; height: 24px; border-width: 3px; }
@keyframes spin { to { transform: rotate(360deg); } }

/* 通用按钮(与 CompactDialog 风格统一) */
.cd-btn {
  font-size: 11px;
  padding: 2px 10px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 22px;
  line-height: 1;
  transition: all 0.15s;
}
.cd-btn:hover { border-color: var(--color-blue); color: var(--color-blue); }
.cd-btn.primary { background: var(--color-blue); border-color: var(--color-blue); color: #fff; }
.cd-btn.primary:hover { opacity: 0.85; }
</style>
