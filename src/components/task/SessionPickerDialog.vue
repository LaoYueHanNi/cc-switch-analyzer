<template>
  <CompactDialog
    :show="show"
    title="添加会话到任务"
    width="520px"
    @update:show="emit('update:show', $event)"
  >
    <!-- 步骤进度条 -->
    <div class="picker-steps">
      <div class="picker-step" :class="{ active: step === 0, done: step > 0 }">
        <span class="step-dot">{{ step > 0 ? '✓' : '1' }}</span>
        <span class="step-label">选择目录</span>
      </div>
      <div class="picker-step-line" :class="{ done: step > 0 }" />
      <div class="picker-step" :class="{ active: step === 1 }">
        <span class="step-dot">2</span>
        <span class="step-label">勾选会话</span>
      </div>
    </div>

    <!-- Step 1: 选择目录(与会话 tab 一级页一致) -->
    <div v-if="step === 0" class="step-body">
      <p v-if="!hasDatabase" class="hint-warn">
        当前未加载数据库,无法列出项目目录。请先在"按模型"页选择数据源。
      </p>
      <p v-else-if="projectGroups.length === 0" class="empty-row">
        数据库中暂无任何项目目录。
      </p>
      <div v-else class="dir-list">
        <div
          v-for="g in projectGroups"
          :key="g.projectDir"
          class="dir-row"
          @click="onPickGroup(g)"
        >
          <span class="dir-name" :title="g.projectDir">{{ g.displayName }}</span>
          <span class="dir-stats">
            <span class="stat-value">{{ g.sessionCount }}</span>
            <span class="stat-label">个会话</span>
          </span>
        </div>
      </div>
    </div>

    <!-- Step 2: 勾选会话(数据已在进入前加载好,无加载态) -->
    <div v-else class="step-body">
      <div v-if="candidateSessions.length === 0" class="empty-row">
        该目录下没有可绑定的会话(已自动过滤掉已绑定的)。
      </div>
      <div v-else class="session-list">
        <label
          v-for="s in candidateSessions"
          :key="s.sessionId"
          class="session-item"
        >
          <input
            type="checkbox"
            class="session-cb"
            :checked="selectedIds.includes(s.sessionId)"
            @change="onToggle(s.sessionId)"
          />
          <div class="session-icon">
            <span v-if="s.sourceType === 'codex'" v-html="codexSvg" class="agent-svg" />
            <span v-else-if="s.sourceType === 'opencode'" v-html="opencodeSvg" class="agent-svg" />
            <span v-else v-html="claudeSvg" class="agent-svg" />
          </div>
          <div class="session-info">
            <div class="session-title-row">
              <span class="session-id">{{ shortSessionId(s.sessionId) }}</span>
              <span v-if="s.title" class="session-title" :title="s.title">{{ s.title }}</span>
            </div>
            <div class="session-meta">
              <span>{{ s.requestCount }} 次请求</span>
              <span>{{ formatDuration(s.durationSec) }}</span>
            </div>
          </div>
          <div class="session-stats">
            <span class="stat-tokens">{{ formatNum(s.totalTokens) }}</span>
            <span v-if="s.totalCost > 0" class="stat-cost">{{ formatCost(s.totalCost) }}</span>
          </div>
        </label>
      </div>
    </div>

    <template #footer>
      <button v-if="step === 1" class="cd-btn" @click="step = 0">上一步</button>
      <div class="footer-right">
        <button class="cd-btn" @click="emit('update:show', false)">取消</button>
        <button
          v-if="step === 1"
          class="cd-btn primary"
          :disabled="selectedIds.length === 0"
          @click="onSubmit"
        >添加{{ selectedIds.length > 0 ? ` (${selectedIds.length})` : '' }} 个会话</button>
      </div>
    </template>
  </CompactDialog>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import CompactDialog from '@/components/common/CompactDialog.vue'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useTaskStore } from '@/stores/task'
import { formatNum, formatCost, formatDuration, shortSessionId } from '@/utils/format'
import type { ProjectGroupStats, ProjectSessionDetail } from '@/platform/types'
import type { TaskSessionInput } from '@/types/task'
import claudeSvg from '@/assets/claude.svg?raw'
import opencodeSvg from '@/assets/opencode.svg?raw'
import codexSvg from '@/assets/codex.svg?raw'

const props = defineProps<{
  show: boolean
  submitting?: boolean
  // 已绑定到本任务的 sessionid 集合,用于过滤
  boundSessionIds?: string[]
}>()
const emit = defineEmits<{
  'update:show': [v: boolean]
  submit: [sessions: TaskSessionInput[]]
}>()

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const taskStore = useTaskStore()
const hasDatabase = computed(() => dbStore.hasDatabase)

const step = ref(0)
const projectGroups = ref<ProjectGroupStats[]>([])
const pickedGroup = ref<ProjectGroupStats | null>(null)
const candidateSessions = ref<ProjectSessionDetail[]>([])
const selectedIds = ref<string[]>([])

watch(
  () => props.show,
  (v) => {
    if (v) {
      step.value = 0
      pickedGroup.value = null
      candidateSessions.value = []
      selectedIds.value = []
      loadGroups()
    }
  }
)

async function loadGroups() {
  if (!hasDatabase.value) {
    projectGroups.value = []
    return
  }
  try {
    projectGroups.value = await platformAdapter.querySessionProjectGroups(
      filterStore.filterParams
    )
  } catch (err) {
    console.error('[SessionPicker] 加载项目目录失败:', err)
    projectGroups.value = []
  }
}

function onPickGroup(g: ProjectGroupStats) {
  pickedGroup.value = g
  loadCandidates()
}

async function loadCandidates() {
  const g = pickedGroup.value
  if (!g || !g.sessionIds.length) {
    candidateSessions.value = []
    step.value = 1
    return
  }
  // 一次性拉完再切到 step 2,避免出现"加载中"中间态
  try {
    const details = await platformAdapter.queryProjectSessionDetails(
      filterStore.filterParams,
      g.sessionIds
    )
    // 过滤掉已绑定的会话
    const bound = new Set(taskStore.currentDetail?.sessions.map(s => s.sessionId) || [])
    candidateSessions.value = details
      .filter(d => !bound.has(d.sessionId))
      .sort((a, b) => b.endTime - a.endTime)
  } catch (err) {
    console.error('[SessionPicker] 加载会话失败:', err)
    candidateSessions.value = []
  }
  step.value = 1
}

function onToggle(sid: string) {
  if (selectedIds.value.includes(sid)) {
    selectedIds.value = selectedIds.value.filter(x => x !== sid)
  } else {
    selectedIds.value = [...selectedIds.value, sid]
  }
}

function onSubmit() {
  const g = pickedGroup.value
  if (!g) return
  const inputs: TaskSessionInput[] = candidateSessions.value
    .filter(s => selectedIds.value.includes(s.sessionId))
    .map(s => ({
      sessionId: s.sessionId,
      source: s.sourceType || '',
      projectDir: s.projectDir || g.projectDir,
      title: s.title || ''
    }))
  emit('submit', inputs)
}
</script>

<style scoped>
.picker-steps {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 12px;
  padding: 4px 0;
}
.picker-step {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--text-muted);
}
.picker-step.active { color: var(--color-blue); font-weight: 600; }
.picker-step.done { color: var(--text-secondary); }
.step-dot {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px; height: 16px;
  border-radius: 50%;
  background: var(--bg-hover);
  color: var(--text-muted);
  font-size: 10px;
  border: 1px solid var(--border-main);
}
.picker-step.active .step-dot {
  background: var(--color-blue);
  color: #fff;
  border-color: var(--color-blue);
}
.picker-step.done .step-dot {
  background: var(--color-green);
  color: #fff;
  border-color: var(--color-green);
}
.picker-step-line {
  flex: 1;
  height: 1px;
  background: var(--border-main);
}
.picker-step-line.done { background: var(--color-green); }

.step-body {
  min-height: 160px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.hint-warn {
  color: var(--text-muted);
  font-size: 11px;
  margin: 4px 0 0;
}
.empty-row {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 32px 0;
  color: var(--text-muted);
  font-size: 12px;
}

/* Step 1: 目录列表(单行) */
.dir-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 360px;
  overflow-y: auto;
  padding: 2px;
}
.dir-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.15s;
  min-width: 0;
}
.dir-row:hover {
  background: var(--bg-hover);
}
.dir-row .dir-name {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dir-row .dir-stats {
  display: flex;
  align-items: baseline;
  gap: 2px;
  flex-shrink: 0;
  font-size: 12px;
}
.dir-row .dir-stats .stat-value {
  font-weight: 600;
  color: var(--color-green);
}
.dir-row .dir-stats .stat-label {
  font-size: 10px;
  color: var(--text-muted);
}

/* Step 2: 会话列表(与会话 tab 二级卡 / 任务详情卡风格统一) */
.session-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 320px;
  overflow-y: auto;
  padding-right: 2px;
}
.session-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border: 1px solid var(--border-light);
  border-radius: 4px;
  transition: border-color 0.15s;
  cursor: pointer;
  min-width: 0;
}
.session-item:hover { border-color: var(--color-blue); }
.session-cb {
  cursor: pointer;
  accent-color: var(--color-blue);
  flex-shrink: 0;
}
.session-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px; height: 18px;
  color: var(--text-tertiary);
  flex-shrink: 0;
}
.agent-svg :deep(svg) {
  width: 14px; height: 14px;
  display: block;
}
.session-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.session-title-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.session-id {
  font-family: monospace;
  font-size: 12px;
  color: var(--text-primary);
  font-weight: 500;
  flex-shrink: 0;
}
.session-title {
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0;
}
.session-meta {
  display: flex;
  gap: 8px;
  font-size: 10px;
  color: var(--text-muted);
}
.session-stats {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 1px;
  flex-shrink: 0;
  text-align: right;
  min-width: 90px;
}
.stat-tokens {
  font-size: 12px;
  color: var(--color-green);
  font-weight: 600;
}
.stat-cost {
  font-size: 13px;
  font-weight: 700;
  color: var(--color-cost);
}

.footer-right {
  display: flex;
  gap: 6px;
  margin-left: auto;
}
.cd-btn {
  font-size: 11px; padding: 2px 10px; border: 1px solid var(--border-main);
  border-radius: 3px; background: transparent; color: var(--text-primary); cursor: pointer;
}
.cd-btn:hover:not(:disabled) { border-color: var(--color-blue); color: var(--color-blue); }
.cd-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.cd-btn.primary { background: var(--color-blue); border-color: var(--color-blue); color: #fff; }
.cd-btn.primary:hover:not(:disabled) { opacity: 0.85; }
</style>
