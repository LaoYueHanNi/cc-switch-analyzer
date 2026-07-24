<template>
  <CompactDialog
    :show="show"
    :title="dialogTitle"
    width="900px"
    :z-index="11000"
    @update:show="emit('update:show', $event)"
  >
    <div class="preview-toolbar">
      <n-switch size="small" :value="filteredOnly" @update:value="onToggleFiltered" />
      <span class="preview-toolbar-label">仅看归因过滤</span>
      <span class="preview-toolbar-label">模型</span>
      <CompactSelect
        :model-value="modelFilter"
        :options="modelOptions"
        clearable
        placeholder="全部"
        @update:model-value="onModelChange"
      />
      <span class="preview-toolbar-count">共 {{ total }} 条</span>
    </div>

    <div class="preview-table-wrap">
      <table class="preview-table">
        <thead>
          <tr>
            <th class="col-time">时间</th>
            <th class="col-account">账号</th>
            <th class="col-model">模型</th>
            <th class="col-num">输入</th>
            <th class="col-num">输出</th>
            <th class="col-num">缓存读</th>
            <th class="col-num">缓存写</th>
            <th class="col-reason">原因</th>
            <th class="col-action">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="loading">
            <td colspan="9" class="empty-cell">加载中…</td>
          </tr>
          <tr v-else-if="items.length === 0">
            <td colspan="9" class="empty-cell">
              {{ emptyHint }}
            </td>
          </tr>
          <tr
            v-for="(row, idx) in items"
            :key="`${row.cachePath || ''}-${row.rowKey || `${row.createdAt}-${row.model}-${idx}`}`"
            :class="{ filtered: row.filtered }"
          >
            <td class="col-time">{{ formatTime(row.createdAt) }}</td>
            <td class="col-account" :title="row.userId || ''">{{ maskUserId(row.userId) }}</td>
            <td class="col-model" :title="row.model">{{ row.model }}</td>
            <td class="col-num" :title="String(row.input)">{{ formatNum(row.input) }}</td>
            <td class="col-num" :title="String(row.output)">{{ formatNum(row.output) }}</td>
            <td class="col-num" :title="String(row.cacheRead)">{{ formatNum(row.cacheRead) }}</td>
            <td class="col-num" :title="String(row.cacheCreation)">{{ formatNum(row.cacheCreation) }}</td>
            <td class="col-reason">
              <span
                v-if="row.override === 'keep'"
                class="reason-tag keep"
                :title="algoReasonTitle(row)"
              >手动保留</span>
              <span
                v-else-if="row.override === 'filter'"
                class="reason-tag filter-ov"
                :title="algoReasonTitle(row)"
              >手动过滤</span>
              <span v-else-if="row.reason" class="reason-tag" :class="row.reason">{{ reasonLabel(row.reason) }}</span>
              <span v-else class="reason-dash">—</span>
            </td>
            <td class="col-action">
              <template v-if="row.override === 'keep'">
                <button
                  type="button"
                  class="act-btn"
                  :disabled="actingKey === actingId(row)"
                  @click="onSetOverride(row, 'filter')"
                >改为过滤</button>
                <button
                  type="button"
                  class="act-btn act-cancel"
                  :disabled="actingKey === actingId(row)"
                  @click="onClearOverride(row)"
                >取消申诉</button>
              </template>
              <template v-else-if="row.override === 'filter'">
                <button
                  type="button"
                  class="act-btn"
                  :disabled="actingKey === actingId(row)"
                  @click="onSetOverride(row, 'keep')"
                >改为取回</button>
                <button
                  type="button"
                  class="act-btn act-cancel"
                  :disabled="actingKey === actingId(row)"
                  @click="onClearOverride(row)"
                >取消申诉</button>
              </template>
              <template v-else-if="row.filtered">
                <button
                  type="button"
                  class="act-btn"
                  :disabled="actingKey === actingId(row)"
                  @click="onSetOverride(row, 'keep')"
                >申诉</button>
              </template>
              <template v-else>
                <button
                  type="button"
                  class="act-btn"
                  :disabled="actingKey === actingId(row)"
                  @click="onSetOverride(row, 'filter')"
                >过滤</button>
              </template>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="preview-pager">
      <button class="pager-btn" :disabled="page <= 1 || loading" @click="goPage(page - 1)">上一页</button>
      <span class="pager-info">第 {{ page }} / {{ totalPages }} 页</span>
      <button class="pager-btn" :disabled="page >= totalPages || loading" @click="goPage(page + 1)">下一页</button>
    </div>
  </CompactDialog>
</template>

<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { NSwitch } from 'naive-ui'
import CompactDialog from '@/components/common/CompactDialog.vue'
import CompactSelect from '@/components/common/CompactSelect.vue'
import { platformAdapter } from '@/platform'
import { formatNum } from '@/utils/format'
import type { CursorCsvPreviewRow, CursorFilterReason, CursorOverrideAction } from '@/platform/types'

const props = defineProps<{
  show: boolean
  initialFilteredOnly?: boolean
  /** 仅预览指定账号缓存 */
  cachePath?: string | null
  userId?: string | null
}>()

const emit = defineEmits<{
  'update:show': [value: boolean]
  'stats-changed': []
}>()

const PAGE_SIZE = 50
const filteredOnly = ref(false)
const modelFilter = ref('')
const availableModels = ref<string[]>([])
const page = ref(1)
const total = ref(0)
const items = ref<CursorCsvPreviewRow[]>([])
const loading = ref(false)
const actingKey = ref('')

const dialogTitle = computed(() => {
  const uid = (props.userId || '').trim()
  if (!uid) return 'Cursor CSV 预览'
  const short = uid.length <= 12 ? uid : `${uid.slice(0, 6)}…${uid.slice(-4)}`
  return `Cursor CSV · ${short}`
})

const totalPages = computed(() => Math.max(1, Math.ceil(total.value / PAGE_SIZE)))

const modelOptions = computed(() =>
  availableModels.value.map((m) => ({ label: m, value: m })),
)

const emptyHint = computed(() => {
  if (filteredOnly.value && modelFilter.value) {
    return `没有被归因过滤的「${modelFilter.value}」记录`
  }
  if (filteredOnly.value) return '没有被归因过滤的记录'
  if (modelFilter.value) return `没有「${modelFilter.value}」的 CSV 数据`
  return '暂无 CSV 数据'
})

function reasonLabel(reason: CursorFilterReason): string {
  if (reason === 'model') return '模型不对'
  if (reason === 'time') return '时间不对'
  return '无本机匹配'
}

function algoReasonTitle(row: CursorCsvPreviewRow): string {
  if (!row.reason) return '原：算法保留'
  return `原：${reasonLabel(row.reason)}`
}

function formatTime(epoch: number): string {
  return new Date(epoch * 1000).toLocaleString()
}

async function loadPage(): Promise<void> {
  loading.value = true
  try {
    const res = await platformAdapter.cursorPreviewCsv(
      page.value,
      PAGE_SIZE,
      filteredOnly.value,
      modelFilter.value || null,
      props.cachePath ?? null,
      props.userId ?? null,
    )
    items.value = res.items
    total.value = res.total
    page.value = res.page
    availableModels.value = res.availableModels ?? []
  } catch (e) {
    console.error('[cursor] preview csv failed:', e)
    items.value = []
    total.value = 0
    availableModels.value = []
  } finally {
    loading.value = false
  }
}

async function onSetOverride(row: CursorCsvPreviewRow, action: CursorOverrideAction): Promise<void> {
  if (!row.rowKey || actingKey.value) return
  actingKey.value = actingId(row)
  try {
    await platformAdapter.cursorSetAttributionOverride(
      row.rowKey,
      action,
      row.createdAt,
      row.model,
      row.cachePath ?? null,
      row.userId ?? null,
    )
    emit('stats-changed')
    await loadPage()
  } catch (e) {
    console.error('[cursor] set attribution override failed:', e)
  } finally {
    actingKey.value = ''
  }
}

async function onClearOverride(row: CursorCsvPreviewRow): Promise<void> {
  if (!row.rowKey || actingKey.value) return
  actingKey.value = actingId(row)
  try {
    await platformAdapter.cursorClearAttributionOverride(
      row.rowKey,
      row.cachePath ?? null,
      row.userId ?? null,
    )
    emit('stats-changed')
    await loadPage()
  } catch (e) {
    console.error('[cursor] clear attribution override failed:', e)
  } finally {
    actingKey.value = ''
  }
}

function actingId(row: CursorCsvPreviewRow): string {
  return `${row.cachePath || row.userId || ''}|${row.rowKey}`
}

function maskUserId(userId?: string | null): string {
  const s = (userId || '').trim()
  if (!s) return '—'
  if (s.length <= 10) return s
  return `${s.slice(0, 6)}…${s.slice(-4)}`
}

function onToggleFiltered(v: boolean): void {
  filteredOnly.value = v
  page.value = 1
  loadPage()
}

function onModelChange(v: string): void {
  modelFilter.value = v
  page.value = 1
  loadPage()
}

function goPage(p: number): void {
  page.value = p
  loadPage()
}

watch(
  () => props.show,
  (visible) => {
    if (!visible) return
    filteredOnly.value = !!props.initialFilteredOnly
    modelFilter.value = ''
    page.value = 1
    loadPage()
  },
)
</script>

<style scoped>
.preview-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}

.preview-toolbar-label {
  font-size: 12px;
  color: var(--text-secondary);
}

.preview-toolbar-count {
  margin-left: auto;
  font-size: 11px;
  color: var(--text-tertiary);
}

.preview-table-wrap {
  max-height: 420px;
  overflow: auto;
  border: 1px solid var(--border-main);
  border-radius: 4px;
}

.preview-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
  table-layout: fixed;
}

.preview-table thead {
  position: sticky;
  top: 0;
  z-index: 1;
}

.preview-table th {
  background: var(--bg-card);
  color: var(--text-tertiary);
  font-weight: 600;
  text-align: left;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-main);
  white-space: nowrap;
}

.preview-table td {
  padding: 5px 8px;
  border-bottom: 1px solid var(--border-light);
  color: var(--text-secondary);
  vertical-align: middle;
}

.preview-table tr.filtered td {
  background: color-mix(in srgb, var(--color-cost, #e17055) 6%, transparent);
}

.col-time {
  width: 148px;
  white-space: nowrap;
}

.col-account {
  width: 88px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 10px;
}

.col-model {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col-num {
  width: 58px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.col-reason {
  width: 84px;
  text-align: center;
}

.col-action {
  width: 118px;
  white-space: nowrap;
}

.empty-cell {
  text-align: center;
  color: var(--text-tertiary);
  padding: 28px 8px !important;
}

.reason-dash {
  color: var(--text-faint);
}

.reason-tag {
  display: inline-block;
  font-size: 10px;
  line-height: 1.4;
  padding: 1px 6px;
  border-radius: 3px;
  white-space: nowrap;
}

.reason-tag.model {
  color: #d63031;
  background: color-mix(in srgb, #d63031 14%, transparent);
}

.reason-tag.time {
  color: #e17055;
  background: color-mix(in srgb, #e17055 14%, transparent);
}

.reason-tag.none {
  color: var(--text-tertiary);
  background: var(--border-light);
}

.reason-tag.keep {
  color: #00b894;
  background: color-mix(in srgb, #00b894 14%, transparent);
}

.reason-tag.filter-ov {
  color: #6c5ce7;
  background: color-mix(in srgb, #6c5ce7 14%, transparent);
}

.act-btn {
  font-size: 10px;
  padding: 1px 5px;
  margin-right: 4px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
}

.act-btn:hover:not(:disabled) {
  border-color: #6c5ce7;
  color: #6c5ce7;
}

.act-btn.act-cancel:hover:not(:disabled) {
  border-color: var(--text-tertiary);
  color: var(--text-secondary);
}

.act-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.preview-pager {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  margin-top: 10px;
}

.pager-btn {
  font-size: 11px;
  padding: 3px 10px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
}

.pager-btn:hover:not(:disabled) {
  border-color: #6c5ce7;
  color: #6c5ce7;
}

.pager-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.pager-info {
  font-size: 11px;
  color: var(--text-tertiary);
}
</style>
