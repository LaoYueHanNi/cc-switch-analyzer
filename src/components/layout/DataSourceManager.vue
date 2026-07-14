<template>
  <n-modal :show="show" @update:show="$emit('update:show', $event)" preset="card" title="数据源管理" size="small" style="max-width: 560px">
    <div class="source-list">
      <div class="source-block" v-for="slot in slots" :key="slot.key">
        <div class="source-item">
          <template v-if="slot.key === 'cursor'">
            <n-switch size="small" :value="slot.enabled" :disabled="!cursorStatus.loggedIn" @update:value="onToggle(slot)" />
            <button v-if="!cursorStatus.loggedIn" class="source-type cursor" @click="showLoginDialog = true">
              登录 Cursor
            </button>
            <button v-else class="source-type cursor" @click="onCursorSync" :disabled="cursorSyncing">
              {{ cursorSyncing ? '同步中...' : 'Cursor' }}
            </button>
            <span class="source-path" :title="cursorStatusText">{{ cursorStatusText }}</span>
            <button v-if="cursorStatus.loggedIn" class="sync-btn" @click="onCursorSync" :disabled="cursorSyncing" title="立即同步">
              ↻
            </button>
            <button v-if="cursorStatus.loggedIn" class="remove-btn" @click="onCursorLogout" title="退出登录">
              <n-icon size="12"><close-outline /></n-icon>
            </button>
          </template>
          <template v-else>
            <n-switch size="small" :value="slot.enabled" :disabled="!slot.path" @update:value="onToggle(slot)" />
            <button class="source-type" :class="slot.key" @click="onSelect(slot.key)">
              {{ slot.label }}
            </button>
            <span class="source-path" :title="slot.path || ''">{{ slot.path || '未选择' }}</span>
            <button v-if="slot.path" class="remove-btn" @click="onRemove(slot.key)" title="移除">
              <n-icon size="12"><close-outline /></n-icon>
            </button>
          </template>
        </div>

        <!-- Cursor 本机精准归因（归属 Cursor） -->
        <div v-if="slot.key === 'cursor' && cursorStatus.loggedIn" class="cursor-attr">
          <div class="cursor-attr-row">
            <n-switch
              size="small"
              :value="!!cursorStatus.attributionEnabled"
              :disabled="attributionToggling"
              @update:value="onToggleAttribution"
            />
            <span class="tm-label">本机精准归因</span>
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

    <!-- TrafficMonitor 插件管理 -->
    <n-divider style="margin: 12px 0 8px" />
    <div class="tm-section">
      <div class="tm-header">TrafficMonitor 插件管理</div>

      <div class="tm-row">
        <n-button size="tiny" :loading="downloading === 'x86'" @click="downloadPlugin('x86')">
          下载 x86 插件
        </n-button>
        <n-button size="tiny" :loading="downloading === 'x64'" @click="downloadPlugin('x64')">
          下载 x64 插件
        </n-button>
        <span class="tm-hint" v-if="downloadedPath">
          已下载至 {{ downloadedPath }}
        </span>
      </div>

      <div class="tm-row">
        <n-switch :value="tmStatus.enabled" @update:value="toggleService" size="small" />
        <span class="tm-label">启用服务</span>
        <span class="tm-hint" v-if="tmStatus.running">
          已启用 · 端口 {{ tmStatus.port }}
        </span>
      </div>
    </div>

    <!-- 更新 -->
    <n-divider style="margin: 12px 0 8px" />
    <div class="tm-section">
      <div class="tm-header">版本更新</div>
      <div class="tm-row">
        <button class="check-update-btn" :disabled="updaterStore.status === 'checking'" @click="updaterStore.checkForUpdate()">
          {{ updaterStore.status === 'checking' ? '检查中...' : '检查更新' }}
        </button>
        <span class="tm-hint" v-if="updaterStore.status === 'idle'">当前版本 v{{ currentVersion }}</span>
        <span class="tm-hint up-to-date" v-if="updaterStore.status === 'upToDate'">已是最新版本</span>
      </div>
      <div class="tm-row">
        <span class="proxy-label">代理</span>
        <input
          class="proxy-input"
          :value="updaterStore.proxy"
          @input="updaterStore.setProxy(($event.target as HTMLInputElement).value)"
          placeholder="http://127.0.0.1:7890"
        />
      </div>
    </div>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NModal, NIcon, NButton, NSwitch, NDivider, NInput } from 'naive-ui'
import { CloseOutline } from '@vicons/ionicons5'
import CursorCsvPreviewDialog from '@/components/layout/CursorCsvPreviewDialog.vue'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'
import { useUpdaterStore } from '@/stores/updater'
import { platformAdapter } from '@/platform'
import { formatNum } from '@/utils/format'
import type { CursorStatusInfo, DefaultPaths, TokenQuad, TmServiceStatus } from '@/platform/types'

const props = defineProps<{ show: boolean }>()
defineEmits<{ 'update:show': [value: boolean] }>()

const emptyQuad = (): TokenQuad => ({ input: 0, output: 0, cacheRead: 0, cacheCreation: 0 })

const dbStore = useDatabaseStore()
const updaterStore = useUpdaterStore()
const currentVersion = ref('')
platformAdapter.getAppVersion().then(v => currentVersion.value = v).catch(() => {})
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

// 弹窗打开时再拉状态，避开启动时 auto_load 尚未完成的竞态
watch(
  () => props.show,
  (visible) => {
    if (visible) {
      loadCursorStatus()
      loadTmStatus()
    }
  },
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

// ===== TrafficMonitor 插件管理 =====

const tmStatus = ref<TmServiceStatus>({ enabled: false, running: false, port: 19810 })
const downloading = ref<string | false>(false)
const downloadedPath = ref('')

async function loadTmStatus(): Promise<void> {
  try {
    tmStatus.value = await platformAdapter.getHttpServiceStatus()
  } catch { /* ignore */ }
}
loadTmStatus()

async function downloadPlugin(arch: 'x86' | 'x64'): Promise<void> {
  downloading.value = arch
  try {
    const path = await platformAdapter.downloadTrafficMonitorPlugin(arch)
    downloadedPath.value = path
  } catch {
    // ignore
  } finally {
    downloading.value = false
  }
}

async function toggleService(enabled: boolean): Promise<void> {
  try {
    tmStatus.value = await platformAdapter.toggleHttpService(enabled)
  } catch {
    // ignore
  }
}
</script>

<style scoped>
.source-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.source-block {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.source-item {
  display: flex;
  align-items: center;
  gap: 8px;
}

.cursor-attr {
  margin-left: 28px;
  padding: 6px 8px;
  border-left: 2px solid #6c5ce7;
  background: color-mix(in srgb, #6c5ce7 8%, transparent);
  border-radius: 0 4px 4px 0;
}

.cursor-attr-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.attr-stats {
  margin-top: 6px;
  display: flex;
  flex-direction: column;
  gap: 3px;
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
  padding: 2px 4px;
  border: none;
  border-radius: 3px;
  background: transparent;
  text-align: left;
  cursor: pointer;
}

.attr-stats-row.clickable:hover {
  background: color-mix(in srgb, #6c5ce7 12%, transparent);
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

.source-type {
  font-size: 10px;
  padding: 3px 8px;
  border: none;
  border-radius: 3px;
  font-weight: 500;
  white-space: nowrap;
  cursor: pointer;
  line-height: 1.4;
}

.source-type.cc-switch {
  background: var(--color-blue);
  color: #fff;
}

.source-type.cc-switch:hover {
  opacity: 0.85;
}

.source-type.opencode {
  background: var(--color-amber);
  color: #fff;
}

.source-type.opencode:hover {
  opacity: 0.85;
}

.source-type.ai-proxy {
  background: var(--color-green);
  color: #fff;
}

.source-type.ai-proxy:hover {
  opacity: 0.85;
}

.source-type.cursor {
  background: #6c5ce7;
  color: #fff;
}

.source-type.cursor:hover {
  opacity: 0.85;
}

.source-type.cursor:disabled {
  opacity: 0.6;
  cursor: wait;
}

.sync-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 14px;
}

.sync-btn:hover:not(:disabled) {
  background: var(--border-light);
  color: var(--text-primary);
}

.sync-btn:disabled {
  opacity: 0.5;
  cursor: wait;
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

.source-path {
  flex: 1;
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.remove-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
}

.remove-btn:hover {
  background: var(--border-light);
  color: var(--color-cost);
}

/* TrafficMonitor 插件管理 */
.tm-section {
  padding: 4px 0;
}

.tm-header {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 8px;
}

.tm-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
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

.check-update-btn {
  font-size: 11px;
  padding: 3px 12px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  line-height: 1.4;
}

.check-update-btn:hover:not(:disabled) {
  border-color: var(--color-blue);
  color: var(--color-blue);
}

.check-update-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.up-to-date {
  color: var(--color-green);
}

.proxy-label {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.proxy-input {
  flex: 1;
  font-size: 11px;
  padding: 2px 6px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: var(--bg-card);
  color: var(--text-primary);
  outline: none;
}

.proxy-input:focus {
  border-color: var(--color-blue);
}

.proxy-input::placeholder {
  color: var(--text-faint);
}
</style>
