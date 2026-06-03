<template>
  <div class="task-view">
    <div class="view-toolbar">
      <button class="cd-btn primary" @click="onCreate">
        <span class="btn-icon">+</span>
        新建任务
      </button>
      <span v-if="store.error" class="error-text">{{ store.error }}</span>
      <div class="toolbar-spacer" />
      <button class="cd-btn" @click="store.fetchAll()">刷新</button>
    </div>

    <div v-if="store.tasks.length === 0" class="tab-empty">
      <p>暂无任务,点击右上角"新建任务"开始</p>
    </div>

    <div v-else class="card-grid">
      <TaskCard
        v-for="t in store.tasks"
        :key="t.id"
        :id="t.id"
        :title="t.title"
        :description="t.description"
        :status="t.status"
        :created-at="t.createdAt"
        :updated-at="t.updatedAt"
        :session-count="t.sessionCount"
        :total-tokens="t.totalTokens"
        :total-cost="t.totalCost"
        @click="onOpen(t.id)"
        @edit="onEdit(t)"
        @delete="onDelete(t)"
        @launch-agent="onLaunchAgent"
        @context-launch-agent="onContextLaunchAgent"
      />
    </div>

    <!-- 任务创建/编辑 -->
    <TaskCreateDialog
      v-model:show="dialogShow"
      :initial="editTarget ? { title: editTarget.title, description: editTarget.description, status: editTarget.status } : null"
      :saving="dialogSaving"
      @save="onSave"
    />

    <!-- 添加会话到任务 -->
    <SessionPickerDialog
      v-model:show="pickerShow"
      :submitting="pickerSaving"
      @submit="onAddSessions"
    />

    <!-- 删除确认 -->
    <div v-if="deleteTarget" class="confirm-overlay" @click.self="deleteTarget = null">
      <div class="confirm-dialog">
        <div class="confirm-title">确认删除</div>
        <p class="confirm-msg">
          确定要删除任务 <strong>{{ deleteTarget.title }}</strong> 吗?任务下的 {{ deleteTarget.sessionCount }} 个会话关联也会一并移除。此操作不可撤销。
        </p>
        <div class="confirm-btns">
          <button class="cd-btn" @click="deleteTarget = null">取消</button>
          <button class="cd-btn danger" @click="doDelete">删除</button>
        </div>
      </div>
    </div>

    <!-- Claude Code 供应商右键菜单 -->
    <Teleport to="body">
      <div v-if="providerDropdown.show" class="provider-ctx-overlay" @click="providerDropdown.show = false" @contextmenu.prevent="providerDropdown.show = false" />
      <div v-if="providerDropdown.show" ref="ctxMenuRef" class="provider-ctx-menu" :style="{ left: providerDropdown.x + 'px', top: providerDropdown.y + 'px' }">
        <div class="provider-ctx-header">选择供应商配置</div>
        <div
          v-for="item in providerDropdown.items"
          :key="item.id"
          class="provider-ctx-item"
          @click="onProviderItemSelect(item.id)"
        >{{ item.name }}</div>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted, nextTick, computed } from 'vue'
import { useRouter } from 'vue-router'
import { useMessage } from 'naive-ui'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { platformAdapter } from '@/platform'
import { useDatabaseStore } from '@/stores/database'
import { useTaskStore } from '@/stores/task'
import type { TaskStatus, TaskWithStats } from '@/types/task'
import TaskCard from '@/components/task/TaskCard.vue'
import TaskCreateDialog from '@/components/task/TaskCreateDialog.vue'
import SessionPickerDialog from '@/components/task/SessionPickerDialog.vue'

defineOptions({ name: 'Task' })

const router = useRouter()
const store = useTaskStore()
const dbStore = useDatabaseStore()
const message = useMessage()

const pickerShow = ref(false)
const pickerSaving = ref(false)

const dialogShow = ref(false)
const dialogSaving = ref(false)
const editTarget = ref<TaskWithStats | null>(null)
const deleteTarget = ref<TaskWithStats | null>(null)

// Claude Code 供应商右键菜单
const ctxMenuRef = ref<HTMLElement | null>(null)
const providerDropdown = reactive({
  show: false,
  x: 0,
  y: 0,
  items: [] as { id: string; name: string }[]
})

const ccswitchDbPath = computed(() =>
  dbStore.sources.find(s => s.dbType === 'CC-Switch')?.path
)

onMounted(() => {
  store.fetchAll()
})

function onCreate() {
  editTarget.value = null
  dialogShow.value = true
}

function onEdit(t: TaskWithStats) {
  editTarget.value = t
  dialogShow.value = true
}

async function onSave(payload: { title: string; description: string; status: TaskStatus }) {
  dialogSaving.value = true
  try {
    if (editTarget.value) {
      await store.update(editTarget.value.id, payload)
      message.success('任务已更新')
    } else {
      const newId = await store.create(payload)
      message.success('任务已创建')
      editTarget.value = store.tasks.find(t => t.id === newId) || null
    }
    dialogShow.value = false
  } catch (e: any) {
    message.error(String(e?.message || e))
  } finally {
    dialogSaving.value = false
  }
}

function onDelete(t: TaskWithStats) {
  deleteTarget.value = t
}

async function doDelete() {
  const t = deleteTarget.value
  if (!t) return
  try {
    await store.remove(t.id)
    message.success('任务已删除')
  } catch (e: any) {
    message.error(String(e?.message || e))
  } finally {
    deleteTarget.value = null
  }
}

function onOpen(taskId: number) {
  router.push({ name: 'task-detail', params: { id: String(taskId) } })
}

async function onLaunchAgent(agent: 'claude' | 'opencode' | 'codex') {
  await pickFolderAndLaunch(agent, undefined)
}

async function onContextLaunchAgent(
  agent: 'claude' | 'opencode' | 'codex',
  event: MouseEvent
) {
  if (agent !== 'claude') {
    await pickFolderAndLaunch(agent, undefined)
    return
  }
  if (!ccswitchDbPath.value) return
  let items: { id: string; name: string }[]
  try {
    const providers = await platformAdapter.getCcswitchProviders(ccswitchDbPath.value)
    items = providers.filter(p => p.hasEnv).map(p => ({ id: p.id, name: p.name }))
  } catch (e: any) {
    console.warn('[Task] 加载供应商失败:', e?.message || e)
    return
  }
  if (items.length === 0) {
    message.info('暂无可用供应商配置')
    return
  }
  providerDropdown.items = items
  nextTick(() => {
    const x = event.clientX
    const y = event.clientY
    const vw = window.innerWidth
    const vh = window.innerHeight
    providerDropdown.x = Math.min(x, vw - 200)
    providerDropdown.y = Math.min(y, vh - 60)
    providerDropdown.show = true
  })
}

async function onProviderItemSelect(providerId: string) {
  providerDropdown.show = false
  await pickFolderAndLaunch('claude', providerId)
}

async function pickFolderAndLaunch(
  agent: 'claude' | 'opencode' | 'codex',
  providerId: string | undefined
) {
  let dir: string | null = null
  try {
    const sel = await openDialog({ directory: true, multiple: false, title: `选择工作目录(${agent})` })
    if (typeof sel === 'string') dir = sel
  } catch (e: any) {
    message.error('选择目录失败: ' + (e?.message || e))
    return
  }
  if (!dir) return
  try {
    await platformAdapter.openTaskAgent(agent, dir, providerId, ccswitchDbPath.value || undefined)
    message.success('已在新终端打开')
  } catch (e: any) {
    message.error('启动终端失败: ' + (e?.message || e))
  }
}
</script>

<style scoped>
.task-view {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}

.view-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 0 10px;
}
.toolbar-spacer { flex: 1; }
.error-text {
  font-size: 12px;
  color: var(--color-cost);
}

.tab-loading, .tab-empty {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  padding: 60px 0; gap: 12px; color: var(--text-muted);
}

.spinner {
  display: inline-block;
  width: 14px; height: 14px;
  border: 2px solid var(--border-main);
  border-top-color: var(--color-blue);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
.spinner-lg { width: 24px; height: 24px; border-width: 3px; }
@keyframes spin { to { transform: rotate(360deg); } }

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: var(--card-gap);
  padding: 4px 0;
  align-items: stretch;
}

/* 通用按钮样式(与 CompactDialog 风格统一) */
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
.cd-btn.danger { color: var(--color-cost); border-color: var(--color-cost); }
.cd-btn.danger:hover { background: var(--color-cost); color: #fff; }
.btn-icon { font-size: 13px; line-height: 1; }

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
</style>

<style>
/* Claude Code 供应商右键菜单(与 SessionAnalysis 保持一致) */
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
  transition: background 0.15s;
}
.provider-ctx-item:hover {
  background: var(--bg-hover);
  color: var(--color-blue);
}
</style>
