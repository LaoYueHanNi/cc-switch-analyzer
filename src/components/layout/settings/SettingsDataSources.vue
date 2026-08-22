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
          :value="cursorAnyEnabled"
          :disabled="!hasCursorAccounts"
          @update:value="onToggleAllCursor"
        />
        <n-switch
          v-else
          size="small"
          :value="slot.enabled"
          :disabled="!slot.path"
          @update:value="onToggle(slot)"
        />
      </div>

      <!-- 路径/状态 + 操作（Cursor 已并入账号行，此处仅其它数据源） -->
      <div v-if="slot.key !== 'cursor'" class="source-path-row">
        <span class="path-label">{{ slot.key === 'dsh' || slot.key === 'minimax' ? '数据目录' : '数据库地址' }}</span>
        <span class="source-path" :title="slot.path || ''">{{ slot.path || '未选择' }}</span>
        <div class="path-actions">
          <button
            type="button"
            class="icon-btn"
            :title="slot.key === 'dsh' || slot.key === 'minimax' ? '立即扫描' : (slot.path ? '更换数据库' : '选择数据库')"
            @click="onSelect(slot.key)"
          >
            <n-icon size="14">
              <sync-outline v-if="slot.key === 'dsh' || slot.key === 'minimax'" />
              <create-outline v-else />
            </n-icon>
          </button>
          <button
            v-if="slot.path && slot.key !== 'dsh' && slot.key !== 'minimax'"
            type="button"
            class="icon-btn danger"
            title="移除"
            @click="onRemove(slot.key)"
          >
            <n-icon size="14"><close-outline /></n-icon>
          </button>
        </div>
      </div>

      <!-- DSH：用量插件（可选）数据源切换 -->
      <div v-if="slot.key === 'dsh'" class="dsh-plugin-section">
        <div class="dsh-plugin-title">用量插件 dsh-token-usage（可选）</div>
        <div class="dsh-plugin-row">
          <span class="path-label">仓库</span>
          <span class="source-path" :title="DSH_PLUGIN_REPO">{{ DSH_PLUGIN_REPO }}</span>
          <div class="path-actions">
            <button type="button" class="icon-btn" title="打开仓库" @click="onOpenPluginRepo">
              <n-icon size="14"><open-outline /></n-icon>
            </button>
          </div>
        </div>
        <div class="dsh-plugin-row">
          <span class="path-label">安装</span>
          <code class="dsh-install-cmd">{{ DSH_INSTALL_CMD }}</code>
          <div class="path-actions">
            <button type="button" class="icon-btn" title="复制安装命令" @click="onCopyInstallCmd">
              <n-icon size="14"><copy-outline /></n-icon>
            </button>
          </div>
        </div>
        <div class="dsh-plugin-row">
          <span class="path-label">数据目录</span>
          <span class="source-path" :title="dshSettings.pluginDataDir || ''">
            {{ dshSettings.pluginDataDir || '~/.dsh/token-usage' }}
          </span>
          <span v-if="dshSettings.customDataDir" class="dsh-custom-tag">自定义</span>
          <div class="path-actions">
            <button
              type="button"
              class="icon-btn"
              title="选择插件数据目录（usage 文件所在目录）"
              :disabled="dshDirBusy"
              @click="onPickDshPluginDir"
            >
              <n-icon size="14"><folder-open-outline /></n-icon>
            </button>
            <button
              v-if="dshSettings.customDataDir"
              type="button"
              class="icon-btn"
              title="恢复默认目录"
              :disabled="dshDirBusy"
              @click="onResetDshPluginDir"
            >
              <n-icon size="14"><refresh-outline /></n-icon>
            </button>
          </div>
        </div>
        <p class="dsh-plugin-status">
          <span :class="['dsh-plugin-state', dshSettings.pluginInstalled ? 'ok' : 'warn']">
            {{ dshSettings.pluginInstalled ? '已安装' : '未安装' }}
          </span>
          <template v-if="dshSettings.pluginInstalled">
            · {{ dshSettings.usageFiles }} 个数据文件 · {{ dshSettings.pluginDataDir }}
          </template>
          <template v-else>
            · {{ dshSettings.pluginDataDir || '~/.dsh/token-usage' }}（安装插件后自动出现）
          </template>
        </p>
        <div class="dsh-plugin-mode-row">
          <n-switch
            size="small"
            :value="dshSettings.usePlugin"
            :disabled="dshModeBusy"
            @update:value="onToggleDshPlugin"
          />
          <span class="tm-label">使用此插件数据</span>
          <span class="tm-hint">{{ dshSettings.usePlugin ? '扫描插件记录（按天 JSONL）' : '扫描会话日志（session.jsonl.zstd）' }}</span>
        </div>
        <p class="dsh-plugin-hint">两种来源按请求 ID 去重，切换不清空已导入数据；未使用插件数据时沿用会话扫描。插件自定义数据目录时在此选择其 usage-*.jsonl 所在目录。</p>
      </div>

      <!-- Cursor：Hook 第一行 + 账号行 -->
      <div v-if="slot.key === 'cursor'" class="cursor-rows">
        <div class="cursor-line" @click="showHookSettings = true">
          <span class="cursor-line-main">
            <span class="cursor-account-id">{{ hookLineText }}</span>
          </span>
          <span class="cursor-line-chevron">›</span>
        </div>

        <div
          v-if="!cursorStatus.loggedIn"
          class="cursor-line"
          @click="showLoginDialog = true"
        >
          <span class="cursor-line-main">
            <span class="cursor-account-id">登录 Cursor</span>
          </span>
          <span class="cursor-line-meta">绑定 token 以同步</span>
          <span class="cursor-line-chevron">›</span>
        </div>

        <div
          v-for="acc in cursorAccounts"
          :key="acc.path"
          class="cursor-line"
          @click="openAccountSettings(acc)"
        >
          <n-switch
            size="small"
            :value="acc.enabled"
            :disabled="!acc.sourceId"
            @click.stop
            @update:value="onToggleCursorAccount(acc)"
          />
          <span class="cursor-line-main">
            <span class="cursor-account-id" :title="acc.userId">{{ maskUserId(acc.userId) }}</span>
            <span v-if="acc.isSyncAccount" class="cursor-account-badge">当前绑定</span>
          </span>
          <span
            v-if="acc.isSyncAccount && acc.lastSync"
            class="cursor-line-meta sync-time"
            :title="formatSyncTime(acc.lastSync)"
          >{{ formatSyncTime(acc.lastSync) }}</span>
          <span class="cursor-line-meta">{{ acc.recordCount }} 条</span>
          <div v-if="acc.isSyncAccount" class="path-actions" @click.stop>
            <button
              type="button"
              class="icon-btn"
              :disabled="cursorSyncing"
              :title="cursorSyncing ? '同步中...' : '立即同步'"
              @click="onCursorSync"
            >
              <n-icon size="14"><sync-outline /></n-icon>
            </button>
            <button
              type="button"
              class="icon-btn danger"
              title="退出登录（保留已下载 CSV）"
              @click="onCursorLogout"
            >
              <n-icon size="14"><close-outline /></n-icon>
            </button>
          </div>
          <span class="cursor-line-chevron">›</span>
        </div>

        <div v-if="!hasCursorAccounts && !cursorStatus.loggedIn" class="cursor-empty-hint">
          登录后同步 CSV，账号会出现在此列表
        </div>
      </div>
    </div>
  </div>

  <CursorCsvPreviewDialog
    v-model:show="showCsvPreview"
    :initial-filtered-only="csvPreviewFilteredOnly"
    :cache-path="csvPreviewCachePath"
    :user-id="csvPreviewUserId"
    @stats-changed="onCsvPreviewStatsChanged"
  />

  <!-- 账号设置 -->
  <CompactDialog
    :show="!!accountSettings"
    :title="accountSettingsTitle"
    width="400px"
    @update:show="(v) => { if (!v) accountSettings = null }"
  >
    <template v-if="accountSettings">
      <div class="acc-dlg">
        <template v-if="accountSettings.isSyncAccount">
          <div class="acc-dlg-row">
            <span class="acc-dlg-k">同步范围</span>
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
            <button
              type="button"
              class="acc-dlg-link"
              :disabled="cursorSyncing"
              @click="onCursorSync"
            >{{ cursorSyncing ? '同步中…' : '立即同步' }}</button>
          </div>
          <p class="acc-dlg-hint">北京时间日历日 · 仅此绑定账号 · 下次同步生效</p>
        </template>
        <p v-else class="acc-dlg-hint flat">离线账号：仅查询已下载 CSV。登录对应 token 后可同步更新。</p>

        <div class="acc-dlg-row">
          <span class="acc-dlg-k">精准归因</span>
          <n-switch
            size="small"
            :value="!!accountSettings.attributionEnabled"
            :disabled="attributionToggling"
            @update:value="onToggleAttribution"
          />
        </div>
        <p class="acc-dlg-hint">仅此账号独立开关 · Hook 过滤此账号 CSV（分钟±5 + 模型家族）</p>

        <div class="acc-dlg-row">
          <span class="acc-dlg-k">归因起始</span>
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
        </div>
        <p class="acc-dlg-hint">北京时间 · 此前记录不过滤</p>

        <div class="acc-dlg-stats">
          <button type="button" class="attr-stats-row clickable" title="预览此账号 CSV" @click="openAccountCsvPreview(false)">
            <span class="attr-stats-label">CSV 总计</span>
            <span class="attr-stats-vals" :title="tokenQuadTitle(accountCsvTotal)">{{ formatTokenQuad(accountCsvTotal) }}</span>
          </button>
          <button type="button" class="attr-stats-row clickable" title="预览此账号归因过滤" @click="openAccountCsvPreview(true)">
            <span class="attr-stats-label">归因过滤</span>
            <span class="attr-stats-vals filtered" :title="tokenQuadTitle(accountFilteredOut)">{{ formatTokenQuad(accountFilteredOut) }}</span>
          </button>
        </div>
      </div>
    </template>
  </CompactDialog>

  <!-- Hook 设置 -->
  <CompactDialog
    :show="showHookSettings"
    title="Hook 本机设置"
    width="400px"
    @update:show="(v) => (showHookSettings = v)"
  >
    <div class="acc-dlg">
      <div class="acc-dlg-row">
        <span class="acc-dlg-k">状态</span>
        <span class="acc-dlg-val">{{ cursorStatus.hookInstalled ? '已安装' : '未安装' }} · {{ cursorStatus.localEventCount ?? 0 }} 条</span>
      </div>
      <p class="acc-dlg-hint">{{ cursorStatus.attributionHint || '多账号共用本机 Hook 日志做归因匹配' }}</p>
      <div v-if="cursorStatus.hookAlert" class="dlg-alert" :class="cursorStatus.hookAlert.level">
        {{ cursorStatus.hookAlert.message }}
      </div>

      <div class="acc-dlg-row">
        <span class="acc-dlg-k">Hook 备份</span>
        <select
          class="lookback-select"
          :value="cursorStatus.hookBackupPeriod || 'daily'"
          :disabled="hookBackupSaving"
          @change="onHookBackupPeriodChange(($event.target as HTMLSelectElement).value)"
        >
          <option value="off">关</option>
          <option value="daily">每天</option>
        </select>
        <button
          type="button"
          class="acc-dlg-link"
          :disabled="hookBackupNowLoading"
          @click="onHookBackupNow"
        >{{ hookBackupNowLoading ? '备份中…' : '立即备份' }}</button>
      </div>
      <p class="acc-dlg-hint">{{ hookBackupStatusText }} · 自动备份成功后会归整源日志</p>

      <div class="acc-dlg-row">
        <span class="acc-dlg-k">归整记录</span>
        <button
          type="button"
          class="acc-dlg-link"
          :disabled="hookMergeNowLoading"
          @click.stop="onHookMergeNow"
        >{{ hookMergeNowLoading ? '归整中…' : '归整 Hook 记录' }}</button>
      </div>
      <p class="acc-dlg-hint">压缩本机 requests.jsonl（同模型短窗合并）；立即备份不会触发归整</p>
      <p v-if="hookDialogFeedback" class="dlg-feedback" :class="hookDialogFeedback.ok ? 'ok' : 'err'">
        {{ hookDialogFeedback.text }}
      </p>
    </div>
  </CompactDialog>

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
import { CloseOutline, CopyOutline, CreateOutline, FolderOpenOutline, OpenOutline, RefreshOutline, SyncOutline } from '@vicons/ionicons5'
import CompactDialog from '@/components/common/CompactDialog.vue'
import CursorCsvPreviewDialog from '@/components/layout/CursorCsvPreviewDialog.vue'
import { useDatabaseStore } from '@/stores/database'
import { useDatabase } from '@/composables/useDatabase'
import { platformAdapter } from '@/platform'
import { formatNum } from '@/utils/format'
import type { CursorAccountInfo, CursorStatusInfo, DefaultPaths, DshSettings, TokenQuad } from '@/platform/types'

const props = defineProps<{ active: boolean }>()

const message = useMessage()

// dsh-token-usage 插件(https://github.com/LaoYueHanNi/dsh-token-usage)
const DSH_PLUGIN_REPO = 'https://github.com/LaoYueHanNi/dsh-token-usage'
const DSH_INSTALL_CMD = 'dsh plugin --profile web add github:LaoYueHanNi/dsh-token-usage'

const emptyQuad = (): TokenQuad => ({ input: 0, output: 0, cacheRead: 0, cacheCreation: 0 })

const dbStore = useDatabaseStore()
const { addDatabase, removeDatabase, refreshAfterToggle } = useDatabase()

const defaultPaths = ref<DefaultPaths>({ ccSwitch: null, opencode: null, aiProxy: null, cursor: null })

const dshSettings = ref<DshSettings>({
  usePlugin: false,
  dataDir: null,
  customDataDir: null,
  pluginDataDir: null,
  pluginInstalled: false,
  usageFiles: 0,
  totalRecords: 0,
})
const dshModeBusy = ref(false)
const dshDirBusy = ref(false)

async function loadDshSettings(): Promise<void> {
  try {
    dshSettings.value = await platformAdapter.getDshSettings()
  } catch { /* ignore */ }
}

async function onOpenPluginRepo(): Promise<void> {
  try {
    await platformAdapter.openPluginRepo()
  } catch (e) {
    console.error('[DSH] 打开插件仓库失败', e)
    message.error(typeof e === 'string' ? e : String((e as any)?.message || e || '打开仓库失败'))
  }
}

async function onCopyInstallCmd(): Promise<void> {
  try {
    await navigator.clipboard.writeText(DSH_INSTALL_CMD)
  } catch {
    // 兜底：临时 textarea + execCommand
    const ta = document.createElement('textarea')
    ta.value = DSH_INSTALL_CMD
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    let ok = false
    try {
      ok = document.execCommand('copy')
    } catch { /* ignore */ }
    document.body.removeChild(ta)
    if (!ok) {
      message.error('复制失败，请手动复制')
      return
    }
  }
  message.success('安装命令已复制')
}

/// DSH 数据来源变化(模式切换/目录切换)后的统一生效流程:
/// 按当前模式扫描入库 → 刷新数据源列表与各视图
async function applyDshSourceChange(): Promise<void> {
  if (dshSettings.value.usePlugin) {
    const result = await platformAdapter.scanDshNow()
    console.log('[DSH] 插件数据扫描完成', result)
  }
  const sources = await platformAdapter.listDatabases()
  dbStore.setSources(sources)
  await refreshAfterToggle()
}

async function onToggleDshPlugin(use: boolean): Promise<void> {
  dshModeBusy.value = true
  try {
    const settings = await platformAdapter.setDshPluginMode(use)
    dshSettings.value = settings
    if (use && !settings.pluginInstalled) {
      message.warning('未检测到插件数据目录，请先安装 dsh-token-usage 插件后重试')
    }
    await applyDshSourceChange()
  } catch (e) {
    console.error('[DSH] 切换插件模式失败', e)
    message.error(typeof e === 'string' ? e : String((e as any)?.message || e || '切换失败'))
    await loadDshSettings()
  } finally {
    dshModeBusy.value = false
  }
}

async function onPickDshPluginDir(): Promise<void> {
  const dir = await platformAdapter.pickDirectory('选择 dsh-token-usage 数据目录')
  if (!dir) return
  dshDirBusy.value = true
  try {
    const settings = await platformAdapter.setDshPluginDataDir(dir)
    dshSettings.value = settings
    if (settings.pluginInstalled && settings.usageFiles === 0) {
      message.warning('该目录下未发现 usage-*.jsonl 数据文件，请确认选择的是插件写入的数据目录')
    } else {
      message.success(`插件数据目录已切换：${settings.pluginDataDir}`)
    }
    await applyDshSourceChange()
  } catch (e) {
    console.error('[DSH] 设置插件数据目录失败', e)
    message.error(typeof e === 'string' ? e : String((e as any)?.message || e || '设置失败'))
    await loadDshSettings()
  } finally {
    dshDirBusy.value = false
  }
}

async function onResetDshPluginDir(): Promise<void> {
  dshDirBusy.value = true
  try {
    const settings = await platformAdapter.setDshPluginDataDir(null)
    dshSettings.value = settings
    message.success(`插件数据目录已恢复默认：${settings.pluginDataDir}`)
    await applyDshSourceChange()
  } catch (e) {
    console.error('[DSH] 恢复默认插件数据目录失败', e)
    message.error(typeof e === 'string' ? e : String((e as any)?.message || e || '恢复失败'))
    await loadDshSettings()
  } finally {
    dshDirBusy.value = false
  }
}

const cursorStatus = ref<CursorStatusInfo>({
  loggedIn: false,
  userId: null,
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
  accounts: [],
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
const hookMergeNowLoading = ref(false)
const hookDialogFeedback = ref<{ ok: boolean; text: string } | null>(null)
const showCsvPreview = ref(false)
const csvPreviewFilteredOnly = ref(false)
const csvPreviewCachePath = ref<string | null>(null)
const csvPreviewUserId = ref<string | null>(null)
const showHookSettings = ref(false)
const accountSettings = ref<CursorAccountInfo | null>(null)

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

const accountCsvTotal = computed(() => accountSettings.value?.attributionStats?.csvTotal ?? emptyQuad())
const accountFilteredOut = computed(() => accountSettings.value?.attributionStats?.filteredOut ?? emptyQuad())

const accountSettingsTitle = computed(() => {
  const acc = accountSettings.value
  if (!acc) return '账号设置'
  return `账号 · ${maskUserId(acc.userId)}`
})

function formatTokenQuad(q: TokenQuad): string {
  return `输入 ${formatNum(q.input)} · 输出 ${formatNum(q.output)} · 缓存读 ${formatNum(q.cacheRead)} · 缓存写 ${formatNum(q.cacheCreation)}`
}

function tokenQuadTitle(q: TokenQuad): string {
  return `输入 ${q.input} · 输出 ${q.output} · 缓存读 ${q.cacheRead} · 缓存写 ${q.cacheCreation}`
}

function openAccountCsvPreview(filteredOnly: boolean): void {
  const acc = accountSettings.value
  if (!acc) return
  csvPreviewFilteredOnly.value = filteredOnly
  csvPreviewCachePath.value = acc.path
  csvPreviewUserId.value = acc.userId
  showCsvPreview.value = true
}

function openAccountSettings(acc: CursorAccountInfo): void {
  // 用最新 status 中的同 path 账号，保证 stats 最新
  const fresh = cursorAccounts.value.find(a => a.path === acc.path) ?? acc
  accountSettings.value = fresh
}

async function onCsvPreviewStatsChanged(): Promise<void> {
  try {
    cursorStatus.value = await platformAdapter.cursorStatus()
  } catch (e) {
    console.error('[cursor] refresh status after override failed:', e)
  }
}

const cursorAccounts = computed(() => cursorStatus.value.accounts ?? [])
const hasCursorAccounts = computed(() => cursorAccounts.value.length > 0)
const cursorAnyEnabled = computed(() => {
  const list = cursorAccounts.value
  if (list.length === 0) return false
  return list.some(a => a.enabled)
})

const hookLineText = computed(() => {
  const status = cursorStatus.value.hookInstalled ? '已安装' : '未安装'
  const n = cursorStatus.value.localEventCount ?? 0
  return `本机 Hook - ${status} ${n} 条`
})

function maskUserId(userId: string): string {
  const s = (userId || '').trim()
  if (s.length <= 10) return s || '—'
  return `${s.slice(0, 6)}…${s.slice(-4)}`
}

function formatSyncTime(epoch: number): string {
  return new Date(epoch * 1000).toLocaleString()
}

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
    // 账号弹窗打开时刷新其中的 stats
    if (accountSettings.value) {
      const path = accountSettings.value.path
      accountSettings.value =
        cursorAccounts.value.find(a => a.path === path) ?? accountSettings.value
    }
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
  const acc = accountSettings.value
  attributionToggling.value = true
  try {
    // 指定账号 path/userId：仅切换该账号的精准归因
    cursorStatus.value = await platformAdapter.cursorToggleAttribution(
      enabled,
      acc?.path ?? null,
      acc?.userId ?? null,
    )
    if (acc) {
      accountSettings.value = cursorAccounts.value.find(a => a.path === acc.path) ?? acc
    }
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
  hookDialogFeedback.value = null
  try {
    const result = await platformAdapter.cursorBackupHooksNow()
    const text = result.message || '备份完成'
    hookDialogFeedback.value = { ok: true, text }
    message.success(text)
    await loadCursorStatus()
  } catch (e) {
    console.error('[cursor] hook backup now failed:', e)
    const text = typeof e === 'string' ? e : String((e as any)?.message || e || '立即备份失败')
    hookDialogFeedback.value = { ok: false, text }
    message.error(text)
  } finally {
    hookBackupNowLoading.value = false
  }
}

async function onHookMergeNow(): Promise<void> {
  hookMergeNowLoading.value = true
  hookDialogFeedback.value = null
  try {
    const result = await platformAdapter.cursorMergeHooksNow()
    const text = result.message || '归整完成'
    hookDialogFeedback.value = { ok: true, text }
    message.success(text)
    await loadCursorStatus()
  } catch (e) {
    console.error('[cursor] hook merge now failed:', e)
    const text = typeof e === 'string' ? e : String((e as any)?.message || e || '归整失败')
    hookDialogFeedback.value = { ok: false, text }
    message.error(text)
  } finally {
    hookMergeNowLoading.value = false
  }
}

watch(
  () => props.active,
  (visible) => {
    if (visible) {
      loadCursorStatus()
      loadDshSettings()
    }
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
    key: 'z-code',
    label: 'ZCode',
    path: dbStore.sources.find(s => s.dbType === 'ZCode')?.path || '',
    defaultPath: defaultPaths.value.zCode,
    sourceId: dbStore.sources.find(s => s.dbType === 'ZCode')?.id || '',
    enabled: dbStore.sources.find(s => s.dbType === 'ZCode')?.enabled ?? true,
  },
  {
    key: 'proma',
    label: 'Proma',
    path: dbStore.sources.find(s => s.dbType === 'Proma')?.path || '',
    defaultPath: defaultPaths.value.proma,
    sourceId: dbStore.sources.find(s => s.dbType === 'Proma')?.id || '',
    enabled: dbStore.sources.find(s => s.dbType === 'Proma')?.enabled ?? true,
  },
  {
    key: 'cursor',
    label: 'Cursor',
    path: cursorStatus.value.cachePath || dbStore.sources.find(s => s.dbType === 'Cursor')?.path || '',
    defaultPath: defaultPaths.value.cursor,
    sourceId: dbStore.sources.find(s => s.dbType === 'Cursor')?.id || '',
    enabled: cursorAnyEnabled.value,
  },
  {
    key: 'dsh',
    label: 'DSH',
    path: (() => {
      const base = defaultPaths.value.dsh || ''
      const src = dbStore.sources.find(s => s.dbType === 'DSH')
      const modeTag = dshSettings.value.usePlugin ? '插件数据' : '会话扫描'
      return src && src.recordCount > 0
        ? base + `  (已导入 ${src.recordCount} 条 · ${modeTag})`
        : base
    })(),
    defaultPath: defaultPaths.value.dsh,
    sourceId: dbStore.sources.find(s => s.dbType === 'DSH')?.id || '',
    enabled: dbStore.sources.find(s => s.dbType === 'DSH')?.enabled ?? true,
  },
  {
    key: 'minimax',
    label: 'MiniMax',
    path: (() => {
      const base = defaultPaths.value.minimax || ''
      const src = dbStore.sources.find(s => s.dbType === 'MiniMax')
      return src && src.recordCount > 0
        ? base + `  (已导入 ${src.recordCount} 条)`
        : base
    })(),
    defaultPath: defaultPaths.value.minimax,
    sourceId: dbStore.sources.find(s => s.dbType === 'MiniMax')?.id || '',
    enabled: dbStore.sources.find(s => s.dbType === 'MiniMax')?.enabled ?? true,
  },
])

// slot key → 后端 dbType 字面量映射
const DB_TYPE_MAP: Record<string, string> = {
  'cc-switch': 'CC-Switch',
  'opencode': 'OpenCode',
  'ai-proxy': 'AI-Proxy',
  'z-code': 'ZCode',
  'proma': 'Proma',
  'dsh': 'DSH',
  'minimax': 'MiniMax',
}

async function onSelect(key: string): Promise<void> {
  // DSH 固定扫描当前模式数据目录(插件数据或会话日志),不走目录选择,直接触发增量扫描
  if (key === 'dsh') {
    try {
      const result = await platformAdapter.scanDshNow()
      console.log('[DSH] 扫描完成', result)
      const sources = await platformAdapter.listDatabases()
      dbStore.setSources(sources)
      await loadDshSettings()
    } catch (e) {
      console.error('[DSH] 扫描失败', e)
    }
    return
  }
  // MiniMax 固定扫描 ~/.minimax/v2/sessions,不走目录选择
  if (key === 'minimax') {
    try {
      const result = await platformAdapter.scanMinimaxNow()
      console.log('[MiniMax] 扫描完成', result)
      const sources = await platformAdapter.listDatabases()
      dbStore.setSources(sources)
    } catch (e) {
      console.error('[MiniMax] 扫描失败', e)
    }
    return
  }
  const slot = slots.value.find(s => s.key === key)
  console.log('[DEBUG] onSelect key=', key, 'slotPath=', slot?.path)
  // Proma 是目录型数据源，走目录选择；其余为 SQLite 文件选择
  const filePath = key === 'proma'
    ? await platformAdapter.pickDirectory('选择 Proma 数据目录')
    : await platformAdapter.pickDatabaseFile(slot?.defaultPath || undefined)
  console.log('[DEBUG] pickDatabaseFile ->', filePath)
  if (!filePath) return
  const dbType = DB_TYPE_MAP[key]
  console.log('[DEBUG] dbType=', dbType)
  if (!dbType) return
  const existing = dbStore.sources.find(s => s.dbType === dbType)
  if (existing) {
    await removeDatabase(existing.id)
  }
  await addDatabase(filePath, dbType)
}

async function onRemove(key: string): Promise<void> {
  const dbType = DB_TYPE_MAP[key]
  if (!dbType) return
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

async function onToggleCursorAccount(acc: CursorAccountInfo): Promise<void> {
  if (!acc.sourceId) return
  try {
    const sources = await platformAdapter.toggleDatabase(acc.sourceId)
    dbStore.setSources(sources)
    await loadCursorStatus()
    await refreshAfterToggle()
  } catch {
    // ignore
  }
}

async function onToggleAllCursor(enabled: boolean): Promise<void> {
  const list = cursorAccounts.value.filter(a => a.sourceId)
  for (const acc of list) {
    if (acc.enabled === enabled) continue
    try {
      const sources = await platformAdapter.toggleDatabase(acc.sourceId)
      dbStore.setSources(sources)
    } catch {
      // ignore
    }
  }
  await loadCursorStatus()
  await refreshAfterToggle()
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

.source-dot.z-code {
  background: #00cec9;
}

.source-dot.proma {
  background: #ff9f43;
}

.source-dot.dsh {
  background: #e84393;
}

.source-dot.minimax {
  background: #2d3436;
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

.cursor-rows {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 2px;
}

.cursor-line {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 4px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
}

.cursor-line:hover {
  background: var(--bg-hover, rgba(255, 255, 255, 0.04));
}

.cursor-line-main {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1;
  min-width: 0;
}

.cursor-line-meta {
  color: var(--text-tertiary);
  font-size: 11px;
  white-space: nowrap;
}

.cursor-line-meta.sync-time {
  max-width: 9.5em;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cursor-line-chevron {
  color: var(--text-tertiary);
  font-size: 14px;
  line-height: 1;
  opacity: 0.7;
}

.cursor-empty-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  padding: 4px 2px;
}

.cursor-account-id {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  color: var(--text-primary);
}

.cursor-account-badge {
  font-size: 10px;
  color: var(--primary-color, #18a058);
  border: 1px solid currentColor;
  border-radius: 3px;
  padding: 0 4px;
  line-height: 1.4;
  flex-shrink: 0;
}

.cursor-account-badge.muted {
  color: var(--text-secondary);
}

.cursor-account-badge.warn {
  color: var(--color-cost, #e17055);
}

.dlg-section {
  margin-bottom: 14px;
}

.dlg-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.dlg-hint {
  margin: 4px 0 0;
  font-size: 11px;
  color: var(--text-tertiary);
  line-height: 1.4;
}

.dlg-value {
  font-size: 12px;
  color: var(--text-secondary);
}

.dlg-actions {
  margin-top: 8px;
}

.dlg-alert {
  margin-top: 8px;
  font-size: 11px;
  padding: 6px 8px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--color-cost, #e17055) 12%, transparent);
  color: var(--text-secondary);
}

.dlg-feedback {
  margin: 8px 0 0;
  font-size: 12px;
  line-height: 1.4;
  padding: 6px 8px;
  border-radius: 4px;
}

.dlg-feedback.ok {
  background: color-mix(in srgb, var(--primary-color, #18a058) 14%, transparent);
  color: var(--text-primary);
}

.dlg-feedback.err {
  background: color-mix(in srgb, var(--color-cost, #e17055) 14%, transparent);
  color: var(--text-primary);
}

.acc-dlg {
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-family: inherit;
}

.acc-dlg-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 24px;
}

.acc-dlg-k {
  flex-shrink: 0;
  width: 64px;
  font-size: 12px;
  font-weight: 400;
  line-height: 1.4;
  color: var(--text-tertiary);
  white-space: nowrap;
}

.acc-dlg-val {
  font-size: 12px;
  font-weight: 400;
  line-height: 1.4;
  color: var(--text-secondary);
}

.acc-dlg-hint {
  margin: -2px 0 2px 72px;
  font-size: 11px;
  font-weight: 400;
  line-height: 1.4;
  color: var(--text-tertiary);
}

.acc-dlg-hint.flat {
  margin-left: 0;
}

.acc-dlg-link {
  margin-left: auto;
  padding: 0;
  border: none;
  background: transparent;
  font: inherit;
  font-size: 12px;
  font-weight: 400;
  line-height: 1.4;
  color: var(--color-blue, #4a90d9);
  cursor: pointer;
  white-space: nowrap;
}

.acc-dlg-link:hover:not(:disabled) {
  text-decoration: underline;
}

.acc-dlg-link:disabled {
  opacity: 0.5;
  cursor: wait;
  text-decoration: none;
}

.acc-dlg-stats {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 4px;
  padding-top: 10px;
  border-top: 1px solid var(--border-main);
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
  width: 64px;
  color: var(--text-tertiary);
  white-space: nowrap;
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

/* ========== DSH 用量插件区块 ========== */

.dsh-plugin-section {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 2px;
  padding: 8px 10px;
  border: 1px dashed var(--border-main);
  border-radius: 6px;
  background: color-mix(in srgb, var(--bg-hover, rgba(255, 255, 255, 0.04)) 40%, transparent);
}

.dsh-plugin-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
}

.dsh-plugin-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 22px;
}

.dsh-install-cmd {
  flex: 1;
  min-width: 0;
  font-size: 10px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  color: var(--text-secondary);
  background: var(--border-light);
  padding: 1px 6px;
  border-radius: 3px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dsh-plugin-status {
  margin: 2px 0 0;
  font-size: 11px;
  color: var(--text-tertiary);
  line-height: 1.5;
  word-break: break-all;
}

.dsh-plugin-state {
  font-weight: 600;
}

.dsh-plugin-state.ok {
  color: var(--primary-color, #18a058);
}

.dsh-plugin-state.warn {
  color: var(--color-cost, #e17055);
}

.dsh-custom-tag {
  flex-shrink: 0;
  font-size: 10px;
  line-height: 1;
  padding: 2px 5px;
  border-radius: 3px;
  color: var(--primary-color, #18a058);
  background: color-mix(in srgb, var(--primary-color, #18a058) 14%, transparent);
}

.dsh-plugin-mode-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 2px;
}

.dsh-plugin-hint {
  margin: 2px 0 0;
  font-size: 10px;
  color: var(--text-tertiary);
  line-height: 1.5;
}
</style>
