<template>
  <div class="session-analysis">
    <div v-if="loadingGroups" class="tab-loading">
      <n-spin size="medium" />
      <p>正在加载项目分组...</p>
    </div>

    <div v-else-if="!dbStore.hasDatabase" class="tab-empty">
      <p>请先选择数据库文件</p>
    </div>

    <div v-else-if="projectGroups.length === 0" class="tab-empty">
      <p>暂无会话数据</p>
    </div>

    <template v-else>
      <!-- 一级：项目卡片网格 -->
      <div v-if="!activeProject" class="card-grid">
        <ProjectCard
          v-for="group in projectGroups"
          :key="group.projectDir"
          :display-name="group.displayName"
          :project-dir="group.projectDir"
          :session-count="group.sessionCount"
          :last-active-at="group.lastAt"
          :total-cost="group.totalCost"
          :total-tokens="group.totalTokens"
          :terminal-active="providerDropdown.show && providerDropdown.projectDir === group.projectDir"
          @click="enterProject(group.projectDir)"
          @terminal="onOpenTerminal"
          @contextTerminal="onContextTerminal"
          @openCodeTerminal="onOpenCodeTerminal"
          @codexTerminal="onOpenCodexTerminal"
        />
      </div>

      <!-- 二级：会话列表 -->
      <div v-else class="session-detail">
        <div class="detail-header">
          <button class="back-btn" @click="leaveProject">&larr; 返回</button>
          <span class="detail-name">{{ activeGroup?.displayName }}</span>
          <span class="detail-path">{{ activeProject }}</span>
          <span v-if="!loadingDetails" class="detail-count">{{ sessionDetails.length }} 个会话</span>
        </div>

        <div v-if="loadingDetails" class="tab-loading" style="padding: 30px 0">
          <n-spin size="medium" />
          <p>正在加载会话详情...</p>
        </div>

        <div v-else class="session-list">
          <div
            v-for="s in sessionDetails"
            :key="s.sessionId"
            class="session-wrap"
          >
            <SessionCard
              :session-id="s.sessionId"
              :title="s.title"
              :project="s.projectDir"
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
              <span v-if="s.sourceType === 'codex'" class="action-terminal action-codex" @click="onResumeCodex(s.sessionId, s.projectDir)" title="恢复 Codex 会话"><span v-html="codexSvg"></span></span>
              <span v-else-if="s.sourceType === 'opencode'" class="action-terminal action-opencode" @click="onResumeOpenCode(s.sessionId, s.projectDir)" title="恢复 OpenCode 会话"><span v-html="opencodeSvg"></span></span>
              <span v-else class="action-terminal" @click="onResume(s.sessionId, s.projectDir)" @contextmenu.prevent="onContextTerminalForSession(s.sessionId, s.projectDir, $event)" title="恢复 Claude 会话（右键选择供应商配置）"><span v-html="claudeSvg"></span></span>
              <!-- TODO: 会话删除功能暂未开放，待 UI 确认后恢复 -->
              <!-- <span v-if="s.sourcePath" class="action-delete" @click="confirmDelete(s)" title="删除会话">删除</span> -->
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- 供应商配置选择右键菜单 -->
    <Teleport to="body">
      <div v-if="providerDropdown.show" class="provider-ctx-overlay" @click="providerDropdown.show = false" @contextmenu.prevent="providerDropdown.show = false" />
      <div v-if="providerDropdown.show" ref="ctxMenuRef" class="provider-ctx-menu" :style="{ left: providerDropdown.x + 'px', top: providerDropdown.y + 'px' }">
        <div class="provider-ctx-header">选择供应商配置</div>
        <div v-for="item in providerDropdown.items" :key="item.id" class="provider-ctx-item" @click="onProviderItemSelect(item.id)">{{ item.name }}</div>
      </div>
    </Teleport>

    <!-- 删除确认 -->
    <div v-if="deleteTarget" class="confirm-overlay" @click.self="deleteTarget = null">
      <div class="confirm-dialog">
        <div class="confirm-title">确认删除</div>
        <p class="confirm-msg">
          确定要删除会话 <strong>{{ deleteTarget.title || deleteTarget.sessionId.slice(0, 8) }}</strong> 吗？此操作不可撤销。
        </p>
        <div class="confirm-btns">
          <button class="confirm-btn cancel" @click="deleteTarget = null">取消</button>
          <button class="confirm-btn danger" @click="doDelete">删除</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
defineOptions({ name: 'SessionAnalysis' })
import { ref, reactive, computed, watch, nextTick, onActivated, onDeactivated } from 'vue'
import { NSpin } from 'naive-ui'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import ProjectCard from '@/components/session/ProjectCard.vue'
import SessionCard from '@/components/session/SessionCard.vue'
import claudeSvg from '@/assets/claude.svg?raw'
import opencodeSvg from '@/assets/opencode.svg?raw'
import codexSvg from '@/assets/codex.svg?raw'
import type { ProjectGroupStats, ProjectSessionDetail } from '@/platform/types'

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()

// CC-Switch 数据库路径
const ccswitchDbPath = computed(() =>
  dbStore.sources.find(s => s.dbType === 'CC-Switch')?.path
)

const ctxMenuRef = ref<HTMLElement | null>(null)

// 供应商配置右键菜单状态
const providerDropdown = reactive({
  show: false,
  x: 0,
  y: 0,
  items: [] as { id: string; name: string }[],
  mode: 'new' as 'new' | 'resume',
  projectDir: '',
  sessionId: '',
})

const projectGroups = ref<ProjectGroupStats[]>([])
const sessionDetails = ref<ProjectSessionDetail[]>([])
const activeProject = ref<string | null>(null)
const deleteTarget = ref<ProjectSessionDetail | null>(null)
const loadingGroups = ref(false)
const loadingDetails = ref(false)
const isActive = ref(true)
const needsRefresh = ref(false)

const activeGroup = computed(() =>
  projectGroups.value.find(g => g.projectDir === activeProject.value)
)

async function loadProjectGroups() {
  loadingGroups.value = true
  try {
    projectGroups.value = await platformAdapter.querySessionProjectGroups(filterStore.filterParams)
  } catch (err: any) {
    console.error('[SessionAnalysis] 项目分组查询失败:', err.message || err)
    projectGroups.value = []
  } finally {
    loadingGroups.value = false
  }
}

async function loadSessionDetails(sessionIds: string[]) {
  loadingDetails.value = true
  try {
    const details = await platformAdapter.queryProjectSessionDetails(
      filterStore.filterParams, sessionIds
    )
    details.sort((a, b) => b.endTime - a.endTime)
    sessionDetails.value = details
  } catch (err: any) {
    console.error('[SessionAnalysis] 会话详情查询失败:', err.message || err)
    sessionDetails.value = []
  } finally {
    loadingDetails.value = false
  }
}

function enterProject(projectDir: string) {
  activeProject.value = projectDir
  sessionDetails.value = []
  const group = projectGroups.value.find(g => g.projectDir === projectDir)
  if (group?.sessionIds.length) loadSessionDetails(group.sessionIds)
}

function leaveProject() {
  activeProject.value = null
  sessionDetails.value = []
}

async function onResume(sessionId: string, projectDir?: string) {
  try { await platformAdapter.resumeClaudeSession(sessionId, projectDir) }
  catch (err: any) { console.error('[SessionAnalysis] 恢复失败:', err.message || err) }
}

async function onResumeOpenCode(sessionId: string, projectDir?: string) {
  try { await platformAdapter.resumeOpenCodeSession(sessionId, projectDir) }
  catch (err: any) { console.error('[SessionAnalysis] 恢复 OpenCode 失败:', err.message || err) }
}

async function onResumeCodex(sessionId: string, projectDir?: string) {
  try { await platformAdapter.resumeCodexSession(sessionId, projectDir) }
  catch (err: any) { console.error('[SessionAnalysis] 恢复 Codex 失败:', err.message || err) }
}

async function onOpenTerminal(projectDir: string) {
  try { await platformAdapter.openClaudeTerminal(projectDir) }
  catch (err: any) { console.error('[SessionAnalysis] 打开终端失败:', err.message || err) }
}

async function onOpenCodeTerminal(projectDir: string) {
  try { await platformAdapter.openOpenCodeTerminal(projectDir) }
  catch (err: any) { console.error('[SessionAnalysis] 打开 OpenCode 终端失败:', err.message || err) }
}

async function onOpenCodexTerminal(projectDir: string) {
  try { await platformAdapter.openCodexTerminal(projectDir) }
  catch (err: any) { console.error('[SessionAnalysis] 打开 Codex 终端失败:', err.message || err) }
}

async function loadProviderItems(): Promise<{ id: string; name: string }[]> {
  const dbPath = ccswitchDbPath.value
  if (!dbPath) return []
  try {
    const providers = await platformAdapter.getCcswitchProviders(dbPath)
    return providers.filter(p => p.hasEnv)
  } catch (err: any) {
    console.error('[SessionAnalysis] 获取供应商列表失败:', err.message || err)
    return []
  }
}

function clampPosition(x: number, y: number): { x: number; y: number } {
  const DROPDOWN_W = 180
  const MARGIN = 8
  const vw = window.innerWidth
  const vh = window.innerHeight
  return {
    x: Math.min(x, vw - DROPDOWN_W - MARGIN),
    y: Math.min(y, vh - MARGIN),
  }
}

function menuPositionFromElement(el: HTMLElement): { x: number; y: number } {
  const rect = el.getBoundingClientRect()
  const zoom = parseFloat(getComputedStyle(document.body).zoom) || 1
  const max_x = window.innerWidth / zoom - 8
  let x = rect.left / zoom
  let y = (rect.bottom + 4) / zoom
  if (x + 130 > max_x) x = Math.max(8, rect.right / zoom - 130)
  return { x, y }
}

function adjustMenuPosition() {
  const menu = ctxMenuRef.value
  if (!menu) return
  const zoom = parseFloat(getComputedStyle(document.body).zoom) || 1
  const max_y = window.innerHeight / zoom - 8
  const bottom = providerDropdown.y + menu.offsetHeight
  if (bottom > max_y) {
    providerDropdown.y = Math.max(8, max_y - menu.offsetHeight)
  }
}

async function onContextTerminal(projectDir: string, event: MouseEvent) {
  if (!ccswitchDbPath.value) return
  const items = await loadProviderItems()
  if (items.length === 0) return
  providerDropdown.items = items
  providerDropdown.mode = 'new'
  providerDropdown.projectDir = projectDir
  providerDropdown.sessionId = ''
  const el = (event.target as HTMLElement).closest('button, .action-terminal') as HTMLElement
  nextTick(() => {
    const pos = el ? menuPositionFromElement(el) : clampPosition(event.clientX, event.clientY)
    providerDropdown.x = pos.x
    providerDropdown.y = pos.y
    providerDropdown.show = true
    nextTick(adjustMenuPosition)
  })
}

async function onContextTerminalForSession(sessionId: string, projectDir: string | undefined, event: MouseEvent) {
  if (!ccswitchDbPath.value) return
  const items = await loadProviderItems()
  if (items.length === 0) return
  providerDropdown.items = items
  providerDropdown.mode = 'resume'
  providerDropdown.projectDir = projectDir || ''
  providerDropdown.sessionId = sessionId
  const el = (event.target as HTMLElement).closest('button, .action-terminal') as HTMLElement
  nextTick(() => {
    const pos = el ? menuPositionFromElement(el) : clampPosition(event.clientX, event.clientY)
    providerDropdown.x = pos.x
    providerDropdown.y = pos.y
    providerDropdown.show = true
    nextTick(adjustMenuPosition)
  })
}

async function onProviderItemSelect(providerId: string) {
  providerDropdown.show = false
  const dbPath = ccswitchDbPath.value!
  try {
    if (providerDropdown.mode === 'new') {
      await platformAdapter.openClaudeTerminalWithProvider(providerDropdown.projectDir, providerId, dbPath)
    } else {
      await platformAdapter.resumeClaudeSessionWithProvider(providerDropdown.sessionId, providerId, dbPath, providerDropdown.projectDir || undefined)
    }
  } catch (err: any) {
    console.error('[SessionAnalysis] 携带配置打开终端失败:', err.message || err)
  }
}

function confirmDelete(s: ProjectSessionDetail) { deleteTarget.value = s }

async function doDelete() {
  const target = deleteTarget.value
  if (!target) return
  try {
    await platformAdapter.deleteClaudeSession(target.sessionId)
    sessionDetails.value = sessionDetails.value.filter(s => s.sessionId !== target.sessionId)
  } catch (err: any) {
    console.error('[SessionAnalysis] 删除失败:', err.message || err)
  } finally {
    deleteTarget.value = null
  }
}

function tryLoadGroups() {
  if (isActive.value) loadProjectGroups()
  else needsRefresh.value = true
}

let filterTimer: ReturnType<typeof setTimeout> | null = null
watch(() => dbStore.hasDatabase, (val) => { if (val) tryLoadGroups() }, { immediate: true })
watch(() => filterStore.filterParams, () => {
  if (filterTimer) clearTimeout(filterTimer)
  filterTimer = setTimeout(async () => {
    if (!dbStore.hasDatabase) return
    if (activeProject.value) {
      await loadProjectGroups()
      const ids = activeGroup.value?.sessionIds || []
      if (ids.length) loadSessionDetails(ids)
      else activeProject.value = null
    } else {
      loadProjectGroups()
    }
  }, 300)
}, { deep: true })
watch(() => dbStore.refreshVersion, () => {
  if (!dbStore.hasDatabase) return
  if (activeProject.value) {
    const ids = activeGroup.value?.sessionIds || []
    if (ids.length) loadSessionDetails(ids)
  } else
    tryLoadGroups()
})

onActivated(() => {
  isActive.value = true
  if (needsRefresh.value) {
    needsRefresh.value = false
    if (activeProject.value) {
      const ids = activeGroup.value?.sessionIds || []
      if (ids.length) loadSessionDetails(ids)
      else loadProjectGroups()
    } else loadProjectGroups()
  }
})
onDeactivated(() => { isActive.value = false })
</script>

<style scoped>
.session-analysis { min-height: 200px; display: flex; flex-direction: column; }

.tab-loading, .tab-empty {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  padding: 60px 0; gap: 12px; color: var(--text-muted);
}

/* 一级：卡片网格 */
.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: var(--card-gap);
  padding: 4px 0;
}

/* 二级：详情头 */
.detail-header {
  display: flex; align-items: center; gap: 8px;
  padding: 2px 0 8px; border-bottom: 1px solid var(--border-light);
  margin-bottom: 10px;
}
.back-btn {
  font-size: 12px; color: var(--color-blue); background: none; border: none;
  cursor: pointer; padding: 2px 8px; border-radius: 3px;
  transition: background var(--transition-speed);
}
.back-btn:hover { background: var(--bg-hover); }
.detail-name { font-size: 14px; font-weight: 600; color: var(--text-primary); }
.detail-path {
  font-size: 11px; color: var(--text-muted); flex: 1; overflow: hidden;
  text-overflow: ellipsis; white-space: nowrap;
}
.detail-count { font-size: 11px; color: var(--text-faint); }

/* 会话列表 */
.session-list { flex: 1; overflow-y: auto; }

/* SessionCard + 操作按钮 */
.session-wrap {
  position: relative;
  margin-bottom: 10px;
}
.wrap-actions {
  position: absolute; top: 8px; right: 8px;
  display: flex; gap: 14px; z-index: 1;
}
.session-wrap:hover .wrap-actions { opacity: 1; }

/* 操作 */
.action-terminal, .action-delete {
  cursor: pointer; font-size: 11px; transition: color var(--transition-speed);
}
.action-terminal {
  display: inline-flex; align-items: center; justify-content: center;
  color: var(--text-tertiary);
}
.action-terminal :deep(svg) {
  width: 14px; height: 14px;
}
.action-terminal:hover {
  color: var(--color-blue);
}
.action-delete {
  color: var(--color-cost); font-size: 11px;
}
.action-delete:hover { opacity: 0.7; }

/* 删除确认弹窗 */
.confirm-overlay {
  position: fixed; inset: 0; background: rgba(0,0,0,0.5);
  display: flex; align-items: center; justify-content: center; z-index: 10000;
}
.confirm-dialog {
  background: var(--bg-card); border-radius: 8px; padding: 20px;
  min-width: 320px; box-shadow: 0 8px 32px rgba(0,0,0,0.3);
}
.confirm-title { font-size: 14px; font-weight: 600; color: var(--text-primary); margin-bottom: 12px; }
.confirm-msg { font-size: 13px; color: var(--text-muted); margin-bottom: 16px; line-height: 1.5; }
.confirm-btns { display: flex; justify-content: flex-end; gap: 8px; }
.confirm-btn { padding: 6px 16px; border-radius: 4px; font-size: 12px; cursor: pointer; border: none; }
.confirm-btn.cancel { background: var(--bg-hover); color: var(--text-primary); }
.confirm-btn.cancel:hover { background: var(--border-main); }
.confirm-btn.danger { background: var(--color-cost); color: #fff; }
.confirm-btn.danger:hover { opacity: 0.85; }
</style>

<style>
/* 供应商配置右键菜单 */
.provider-ctx-overlay {
  position: fixed; inset: 0; z-index: 9999;
}
.provider-ctx-menu {
  position: fixed; z-index: 10000;
  min-width: 120px; max-width: 220px;
  max-height: 280px; overflow-y: auto;
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 6px;
  box-shadow: var(--shadow-card);
  padding: 3px 0;
  font-size: 11px;
}
.provider-ctx-header {
  padding: 3px 10px;
  font-size: 10px;
  color: var(--text-muted);
  user-select: none;
}
.provider-ctx-item {
  padding: 4px 10px;
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 3px;
  margin: 0 3px;
  transition: background var(--transition-speed);
}
.provider-ctx-item:hover {
  background: var(--bg-hover);
  color: var(--color-blue);
}
</style>

