<template>
  <div class="source-list">
    <div class="source-block" v-for="slot in slots" :key="slot.key">
      <!-- 名称 + 开关 -->
      <div class="source-header">
        <span class="source-dot" :class="slot.key" />
        <span class="source-name">{{ slot.label }}</span>
        <n-switch
          v-if="slot.key === 'cursor'"
          size="small"
          :value="slot.enabled"
          :disabled="!cursorStatus.loggedIn"
          @update:value="onToggle(slot)"
        />
        <n-switch
          v-else
          size="small"
          :value="slot.enabled"
          :disabled="!slot.path"
          @update:value="onToggle(slot)"
        />
      </div>

      <!-- 路径/状态 + 操作 -->
      <div class="source-path-row">
        <template v-if="slot.key === 'cursor'">
          <span class="path-label">状态</span>
          <span class="source-path" :title="cursorStatusText">{{ cursorStatusText }}</span>
          <div class="path-actions">
            <button
              v-if="!cursorStatus.loggedIn"
              type="button"
              class="icon-btn"
              title="登录 Cursor"
              @click="showLoginDialog = true"
            >
              <n-icon size="14"><create-outline /></n-icon>
            </button>
            <button
              v-else
              type="button"
              class="icon-btn"
              :disabled="cursorSyncing"
              :title="cursorSyncing ? '同步中...' : '立即同步'"
              @click="onCursorSync"
            >
              <n-icon size="14"><sync-outline /></n-icon>
            </button>
            <button
              v-if="cursorStatus.loggedIn"
              type="button"
              class="icon-btn danger"
              title="退出登录"
              @click="onCursorLogout"
            >
              <n-icon size="14"><close-outline /></n-icon>
            </button>
          </div>
        </template>
        <template v-else>
          <span class="path-label">数据库地址</span>
          <span class="source-path" :title="slot.path || ''">{{ slot.path || '未选择' }}</span>
          <div class="path-actions">
            <button
              type="button"
              class="icon-btn"
              :title="slot.path ? '更换数据库' : '选择数据库'"
              @click="onSelect(slot.key)"
            >
              <n-icon size="14"><create-outline /></n-icon>
            </button>
            <button
              v-if="slot.path"
              type="button"
              class="icon-btn danger"
              title="移除"
              @click="onRemove(slot.key)"
            >
              <n-icon size="14"><close-outline /></n-icon>
            </button>
          </div>
        </template>
      </div>

      <!-- Cursor 归因子设置（普通下级，无引用块样式） -->
      <div v-if="slot.key === 'cursor' && cursorStatus.loggedIn" class="cursor-attr">
        <div class="cursor-attr-row">
          <span class="tm-label">本机精准归因</span>
          <n-switch
            size="small"
            :value="!!cursorStatus.attributionEnabled"
            :disabled="attributionToggling"
            @update:value="onToggleAttribution"
          />
          <span class="tm-hint">{{ cursorStatus.attributionHint || '按本机 Hook 过滤（分钟±5 + 模型家族）' }}</span>
        </div>
        <div class="attr-stats">
          <button type="button" class="attr-stats-row clickable" title="预览 CSV 全量" @click="openCsvPreview(false)">
            <span class="attr-stats-label">CSV 总计</span>
            <span class="attr-stats-vals" :title="tokenQuadTitle(csvTotal)">{{ formatTokenQuad(csvTotal) }}</span>
          </button>
          <button type="button" class="attr-stats-row clickable" title="预览被归因过滤的记录" @click="openCsvPreview(true)">
            <span class="attr-stats-label">归因过滤</span>
            <span class="attr-stats-vals filtered" :title="tokenQuadTitle(filteredOut)">{{ formatTokenQuad(filteredOut) }}</span>
          </button>
        </div>
      </div>
    </div>
  </div>

  <CursorCsvPreviewDialog
    v-model:show="showCsvPreview"
    :initial-filtered-only="csvPreviewFilteredOnly"
  />

  <n-modal v-model:show="showLoginDialog" preset="card" title="登录 Cursor" size="small" style="max-width: 420px">
    <p class="login-hint">
      在浏览器打开 cursor.com 并登录，从 DevTools → Application → Cookies 复制
      <code>WorkosCursorSessionToken</code> 的值粘贴到下方。
    </p>
    <n-input
      v-model:value="sessionToken"
      type="textarea"
      placeholder="粘贴 WorkosCursorSessionToken"
      :rows="3"
    />
    <div class="login-actions">
      <n-button size="small" :loading="loginLoading" :disabled="!sessionToken.trim()" @click="onCursorLogin">
        登录并同步
      </n-button>
    </div>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NModal, NIcon, NButton, NSwitch, NInput } from 'naive-ui'
import { CloseOutline, CreateOutline, SyncOutline } from '@vicons/ionicons5'
import CursorCsvPreviewDialog from '@/components/layout/CursorCsvPreviewDialog.vue'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'
import { platformAdapter } from '@/platform'
import { formatNum } from '@/utils/format'
import type { CursorStatusInfo, DefaultPaths, TokenQuad } from '@/platform/types'

const props = defineProps<{ active: boolean }>()

const emptyQuad = (): TokenQuad => ({ input: 0, output: 0, cacheRead: 0, cacheCreation: 0 })

const dbStore = useDatabaseStore()
const { addDatabase, removeDatabase, refreshAfterToggle } = useDatabase()

const defaultPaths = ref<DefaultPaths>({ ccSwitch: null, opencode: null, aiProxy: null, cursor: null })

const cursorStatus = ref<CursorStatusInfo>({
  loggedIn: false,
  lastSync: null,
  recordCount: 0,
  cachePath: null,
  attributionEnabled: false,
  hookInstalled: false,
  localEventCount: 0,
  attributionHint: '',
  attributionStats: { csvTotal: emptyQuad(), filteredOut: emptyQuad() },
})
const showLoginDialog = ref(false)
const sessionToken = ref('')
const loginLoading = ref(false)
const cursorSyncing = ref(false)
const attributionToggling = ref(false)
const showCsvPreview = ref(false)
const csvPreviewFilteredOnly = ref(false)

const csvTotal = computed(() => cursorStatus.value.attributionStats?.csvTotal ?? emptyQuad())
const filteredOut = computed(() => cursorStatus.value.attributionStats?.filteredOut ?? emptyQuad())

function formatTokenQuad(q: TokenQuad): string {
  return `输入 ${formatNum(q.input)} · 输出 ${formatNum(q.output)} · 缓存读 ${formatNum(q.cacheRead)} · 缓存写 ${formatNum(q.cacheCreation)}`
}

function tokenQuadTitle(q: TokenQuad): string {
  return `输入 ${q.input} · 输出 ${q.output} · 缓存读 ${q.cacheRead} · 缓存写 ${q.cacheCreation}`
}

function openCsvPreview(filteredOnly: boolean): void {
  csvPreviewFilteredOnly.value = filteredOnly
  showCsvPreview.value = true
}

const cursorStatusText = computed(() => {
  if (!cursorStatus.value.loggedIn) return '未登录'
  const parts: string[] = []
  if (cursorStatus.value.recordCount > 0) {
    parts.push(`${cursorStatus.value.recordCount} 条记录`)
  }
  if (cursorStatus.value.lastSync) {
    const d = new Date(cursorStatus.value.lastSync * 1000)
    parts.push(`同步于 ${d.toLocaleString()}`)
  }
  return parts.join(' · ') || '已登录'
})

async function loadCursorStatus(): Promise<void> {
  try {
    cursorStatus.value = await platformAdapter.cursorStatus()
  } catch { /* ignore */ }
}

async function onCursorLogin(): Promise<void> {
  const token = sessionToken.value.trim()
  if (!token) return
  loginLoading.value = true
  try {
    const sources = await platformAdapter.cursorLogin(token)
    dbStore.setSources(sources)
    showLoginDialog.value = false
    sessionToken.value = ''
    await loadCursorStatus()
    await refreshAfterToggle()
  } catch (e) {
    console.error('[cursor] login failed:', e)
  } finally {
    loginLoading.value = false
  }
}

async function onCursorSync(): Promise<void> {
  cursorSyncing.value = true
  try {
    await platformAdapter.cursorSync()
    const sources = await platformAdapter.listDatabases()
    dbStore.setSources(sources)
    await loadCursorStatus()
    await refreshAfterToggle()
  } catch (e) {
    console.error('[cursor] sync failed:', e)
  } finally {
    cursorSyncing.value = false
  }
}

async function onCursorLogout(): Promise<void> {
  try {
    const sources = await platformAdapter.cursorLogout(false)
    dbStore.setSources(sources)
    await loadCursorStatus()
    await refreshAfterToggle()
  } catch { /* ignore */ }
}

async function onToggleAttribution(enabled: boolean): Promise<void> {
  attributionToggling.value = true
  try {
    cursorStatus.value = await platformAdapter.cursorToggleAttribution(enabled)
    const sources = await platformAdapter.listDatabases()
    dbStore.setSources(sources)
    await refreshAfterToggle()
  } catch (e) {
    console.error('[cursor] toggle attribution failed:', e)
    await loadCursorStatus()
  } finally {
    attributionToggling.value = false
  }
}

watch(
  () => props.active,
  (visible) => {
    if (visible) loadCursorStatus()
  },
  { immediate: true },
)

async function loadDefaultPaths(): Promise<void> {
  try {
    defaultPaths.value = await platformAdapter.getDefaultPaths()
  } catch { /* ignore */ }
}
loadDefaultPaths()

const slots = computed(() => [
  {
    key: 'cc-switch',
    label: 'CC-Switch',
    path: dbStore.sources.find(s => s.dbType === 'CC-Switch')?.path || '',
    defaultPath: defaultPaths.value.ccSwitch,
    sourceId: dbStore.sources.find(s => s.dbType === 'CC-Switch')?.id || '',
    enabled: dbStore.sources.find(s => s.dbType === 'CC-Switch')?.enabled ?? true,
  },
  {
    key: 'opencode',
    label: 'OpenCode',
    path: dbStore.sources.find(s => s.dbType === 'OpenCode')?.path || '',
    defaultPath: defaultPaths.value.opencode,
    sourceId: dbStore.sources.find(s => s.dbType === 'OpenCode')?.id || '',
    enabled: dbStore.sources.find(s => s.dbType === 'OpenCode')?.enabled ?? true,
  },
  {
    key: 'ai-proxy',
    label: 'AI-Proxy',
    path: dbStore.sources.find(s => s.dbType === 'AI-Proxy')?.path || '',
    defaultPath: defaultPaths.value.aiProxy,
    sourceId: dbStore.sources.find(s => s.dbType === 'AI-Proxy')?.id || '',
    enabled: dbStore.sources.find(s => s.dbType === 'AI-Proxy')?.enabled ?? true,
  },
  {
    key: 'cursor',
    label: 'Cursor',
    path: dbStore.sources.find(s => s.dbType === 'Cursor')?.path || cursorStatus.value.cachePath || '',
    defaultPath: defaultPaths.value.cursor,
    sourceId: dbStore.sources.find(s => s.dbType === 'Cursor')?.id || '',
    enabled: dbStore.sources.find(s => s.dbType === 'Cursor')?.enabled ?? true,
  },
])

async function onSelect(key: string): Promise<void> {
  const slot = slots.value.find(s => s.key === key)
  const filePath = await platformAdapter.pickDatabaseFile(slot?.defaultPath || undefined)
  if (!filePath) return
  const dbType = key === 'cc-switch' ? 'CC-Switch' : key === 'opencode' ? 'OpenCode' : 'AI-Proxy'
  const existing = dbStore.sources.find(s => s.dbType === dbType)
  if (existing) {
    await removeDatabase(existing.id)
  }
  await addDatabase(filePath)
}

async function onRemove(key: string): Promise<void> {
  const dbType = key === 'cc-switch' ? 'CC-Switch' : key === 'opencode' ? 'OpenCode' : 'AI-Proxy'
  const src = dbStore.sources.find(s => s.dbType === dbType)
  if (src) {
    await removeDatabase(src.id)
  }
}

async function onToggle(slot: { sourceId: string }): Promise<void> {
  if (!slot.sourceId) return
  try {
    const sources = await platformAdapter.toggleDatabase(slot.sourceId)
    dbStore.setSources(sources)
    await refreshAfterToggle()
  } catch {
    // ignore
  }
}
</script>

<style scoped>
.source-list {
  display: flex;
  flex-direction: column;
  gap: 0;
}

.source-block {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 14px 0;
  border-bottom: 1px solid var(--border-main);
}

.source-block:first-child {
  padding-top: 0;
}

.source-block:last-child {
  border-bottom: none;
  padding-bottom: 0;
}

.source-header {
  display: flex;
  align-items: center;
  gap: 8px;
}

.source-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.source-dot.cc-switch {
  background: var(--color-blue);
}

.source-dot.opencode {
  background: var(--color-amber);
}

.source-dot.ai-proxy {
  background: var(--color-green);
}

.source-dot.cursor {
  background: #6c5ce7;
}

.source-name {
  flex: 1;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.source-path-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 24px;
}

.path-label {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-tertiary);
  white-space: nowrap;
}

.path-label::after {
  content: '：';
}

.source-path {
  flex: 1;
  min-width: 0;
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.path-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
}

.icon-btn:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--color-blue);
}

.icon-btn.danger:hover:not(:disabled) {
  color: var(--color-cost);
}

.icon-btn:disabled {
  opacity: 0.45;
  cursor: wait;
}

.cursor-attr {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-top: 2px;
}

.cursor-attr-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.attr-stats {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.attr-stats-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 11px;
  line-height: 1.4;
}

.attr-stats-row.clickable {
  width: 100%;
  margin: 0;
  padding: 3px 4px;
  border: none;
  border-radius: 3px;
  background: transparent;
  text-align: left;
  cursor: pointer;
}

.attr-stats-row.clickable:hover {
  background: var(--bg-hover);
}

.attr-stats-label {
  flex-shrink: 0;
  width: 52px;
  color: var(--text-tertiary);
}

.attr-stats-vals {
  color: var(--text-secondary);
  word-break: break-all;
}

.attr-stats-vals.filtered {
  color: var(--color-cost, #e17055);
}

.login-hint {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
  margin: 0 0 10px;
}

.login-hint code {
  font-size: 11px;
  background: var(--border-light);
  padding: 1px 4px;
  border-radius: 2px;
}

.login-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 12px;
}

.tm-label {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.tm-hint {
  font-size: 11px;
  color: var(--text-tertiary);
}
</style>
