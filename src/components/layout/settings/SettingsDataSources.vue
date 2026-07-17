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

      <div v-if="slot.key === 'cursor' && cursorStatus.loggedIn" class="sync-lookback-row">
        <span class="tm-label">同步范围</span>
        <select
          class="lookback-select"
          :value="cursorStatus.syncLookback || '7d'"
          :disabled="lookbackSaving"
          @change="onLookbackChange(($event.target as HTMLSelectElement).value)"
        >
          <option value="1d">1 天</option>
          <option value="7d">7 天</option>
          <option value="30d">30 天</option>
          <option value="all">全部</option>
        </select>
        <span class="tm-hint">按北京时间日历日 · 下次同步生效</span>
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
        <div class="sync-lookback-row">
          <span class="tm-label">归因起始</span>
          <input
            class="lookback-select filter-start-date"
            type="text"
            inputmode="numeric"
            placeholder="YYYY-MM-DD"
            maxlength="10"
            :value="filterStartDateDraft"
            :disabled="filterStartSaving"
            @input="filterStartDateDraft = ($event.target as HTMLInputElement).value"
            @change="commitFilterStart"
            @keydown.enter="($event.target as HTMLInputElement).blur()"
          />
          <input
            class="lookback-select filter-start-time"
            type="text"
            inputmode="numeric"
            placeholder="HH:mm"
            maxlength="5"
            :value="filterStartTimeDraft"
            :disabled="filterStartSaving"
            @input="filterStartTimeDraft = ($event.target as HTMLInputElement).value"
            @change="commitFilterStart"
            @keydown.enter="($event.target as HTMLInputElement).blur()"
          />
          <span class="tm-hint">北京时间 · 此前记录不过滤</span>
        </div>
        <div class="sync-lookback-row">
          <span class="tm-label">Hook 备份</span>
          <select
            class="lookback-select"
            :value="cursorStatus.hookBackupPeriod || 'daily'"
            :disabled="hookBackupSaving"
            @change="onHookBackupPeriodChange(($event.target as HTMLSelectElement).value)"
          >
            <option value="off">关</option>
            <option value="daily">每天</option>
          </select>
          <n-button
            size="tiny"
            secondary
            :loading="hookBackupNowLoading"
            @click="onHookBackupNow"
          >
            立即备份
          </n-button>
          <span class="tm-hint">{{ hookBackupStatusText }}</span>
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
    @stats-changed="onCsvPreviewStatsChanged"
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
import { NModal, NIcon, NButton, NSwitch, NInput, useMessage } from 'naive-ui'
import { CloseOutline, CreateOutline, SyncOutline } from '@vicons/ionicons5'
import CursorCsvPreviewDialog from '@/components/layout/CursorCsvPreviewDialog.vue'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'
import { platformAdapter } from '@/platform'
import { formatNum } from '@/utils/format'
import type { CursorStatusInfo, DefaultPaths, TokenQuad } from '@/platform/types'

const props = defineProps<{ active: boolean }>()

const message = useMessage()

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
  attributionFilterStart: 1_783_927_800,
  syncLookback: '7d',
  hookBackupPeriod: 'daily',
  hookBackupCount: 0,
  hookLastBackupAt: null,
})
const showLoginDialog = ref(false)
const sessionToken = ref('')
const loginLoading = ref(false)
const cursorSyncing = ref(false)
const attributionToggling = ref(false)
const lookbackSaving = ref(false)
const filterStartSaving = ref(false)
const hookBackupSaving = ref(false)
const hookBackupNowLoading = ref(false)
const showCsvPreview = ref(false)
const csvPreviewFilteredOnly = ref(false)

const BJ_OFFSET_SEC = 8 * 3600

function bjPartsFromEpoch(epochSec: number): { date: string; time: string } {
  const d = new Date((epochSec + BJ_OFFSET_SEC) * 1000)
  const y = d.getUTCFullYear()
  const m = String(d.getUTCMonth() + 1).padStart(2, '0')
  const day = String(d.getUTCDate()).padStart(2, '0')
  const h = String(d.getUTCHours()).padStart(2, '0')
  const min = String(d.getUTCMinutes()).padStart(2, '0')
  return { date: `${y}-${m}-${day}`, time: `${h}:${min}` }
}

function bjPartsToEpoch(date: string, time: string): number | null {
  const dm = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date.trim())
  const tm = /^(\d{1,2}):(\d{2})$/.exec(time.trim())
  if (!dm || !tm) return null
  const hour = +tm[1]
  const minute = +tm[2]
  if (hour > 23 || minute > 59) return null
  const month = +dm[2]
  const day = +dm[3]
  if (month < 1 || month > 12 || day < 1 || day > 31) return null
  const utcMs = Date.UTC(+dm[1], month - 1, day, hour, minute)
  return Math.floor(utcMs / 1000) - BJ_OFFSET_SEC
}

const filterStartDateDraft = ref('2026-07-13')
const filterStartTimeDraft = ref('15:30')

function syncFilterStartDraftsFromStatus(): void {
  const parts = bjPartsFromEpoch(cursorStatus.value.attributionFilterStart ?? 1_783_927_800)
  filterStartDateDraft.value = parts.date
  filterStartTimeDraft.value = parts.time
}

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

async function onCsvPreviewStatsChanged(): Promise<void> {
  try {
    cursorStatus.value = await platformAdapter.cursorStatus()
  } catch (e) {
    console.error('[cursor] refresh status after override failed:', e)
  }
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

const hookBackupStatusText = computed(() => {
  const count = cursorStatus.value.hookBackupCount ?? 0
  const last = cursorStatus.value.hookLastBackupAt
  if (count <= 0 && !last) return '尚无备份 · 同步成功后顺带备份'
  const parts: string[] = [`${count} 份`]
  if (last) {
    parts.push(`最近 ${new Date(last * 1000).toLocaleString()}`)
  }
  return parts.join(' · ')
})

async function loadCursorStatus(): Promise<void> {
  try {
    cursorStatus.value = await platformAdapter.cursorStatus()
    syncFilterStartDraftsFromStatus()
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

async function commitFilterStart(): Promise<void> {
  const epoch = bjPartsToEpoch(filterStartDateDraft.value, filterStartTimeDraft.value)
  if (epoch == null) {
    message.error('格式：YYYY-MM-DD 与 HH:mm')
    syncFilterStartDraftsFromStatus()
    return
  }
  if (epoch === (cursorStatus.value.attributionFilterStart ?? 0)) {
    syncFilterStartDraftsFromStatus()
    return
  }
  filterStartSaving.value = true
  try {
    cursorStatus.value = await platformAdapter.cursorSetAttributionFilterStart(epoch)
    syncFilterStartDraftsFromStatus()
    await refreshAfterToggle()
  } catch (e) {
    console.error('[cursor] set attribution filter start failed:', e)
    message.error(typeof e === 'string' ? e : String((e as any)?.message || e || '保存归因起始时间失败'))
    await loadCursorStatus()
  } finally {
    filterStartSaving.value = false
  }
}

async function onLookbackChange(lookback: string): Promise<void> {
  lookbackSaving.value = true
  try {
    cursorStatus.value = await platformAdapter.cursorSetSyncLookback(lookback)
  } catch (e) {
    console.error('[cursor] set sync lookback failed:', e)
    await loadCursorStatus()
  } finally {
    lookbackSaving.value = false
  }
}

async function onHookBackupPeriodChange(period: string): Promise<void> {
  hookBackupSaving.value = true
  try {
    cursorStatus.value = await platformAdapter.cursorSetHookBackupPeriod(period)
  } catch (e) {
    console.error('[cursor] set hook backup period failed:', e)
    message.error(typeof e === 'string' ? e : String((e as any)?.message || e || '保存 Hook 备份周期失败'))
    await loadCursorStatus()
  } finally {
    hookBackupSaving.value = false
  }
}

async function onHookBackupNow(): Promise<void> {
  hookBackupNowLoading.value = true
  try {
    const result = await platformAdapter.cursorBackupHooksNow()
    message.success(result.message || '备份完成')
    await loadCursorStatus()
  } catch (e) {
    console.error('[cursor] hook backup now failed:', e)
    message.error(typeof e === 'string' ? e : String((e as any)?.message || e || '立即备份失败'))
  } finally {
    hookBackupNowLoading.value = false
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

.sync-lookback-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.lookback-select {
  font-size: 11px;
  padding: 2px 6px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: var(--bg-card);
  color: var(--text-primary);
  outline: none;
  cursor: pointer;
}

.lookback-select:focus {
  border-color: var(--color-blue);
}

.lookback-select:disabled {
  opacity: 0.5;
  cursor: wait;
}

.filter-start-date {
  width: 96px;
  cursor: text;
}

.filter-start-time {
  width: 52px;
  cursor: text;
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
