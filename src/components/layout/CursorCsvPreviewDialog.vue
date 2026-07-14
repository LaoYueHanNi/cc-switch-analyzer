<template>
  <CompactDialog :show="show" title="Cursor CSV 预览" width="760px" @update:show="emit('update:show', $event)">
    <div class="preview-toolbar">
      <n-switch size="small" :value="filteredOnly" @update:value="onToggleFiltered" />
      <span class="preview-toolbar-label">仅看归因过滤</span>
      <span class="preview-toolbar-count">共 {{ total }} 条</span>
    </div>

    <div class="preview-table-wrap">
      <table class="preview-table">
        <thead>
          <tr>
            <th class="col-time">时间</th>
            <th class="col-model">模型</th>
            <th class="col-num">输入</th>
            <th class="col-num">输出</th>
            <th class="col-num">缓存读</th>
            <th class="col-num">缓存写</th>
            <th class="col-reason">原因</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="loading">
            <td colspan="7" class="empty-cell">加载中…</td>
          </tr>
          <tr v-else-if="items.length === 0">
            <td colspan="7" class="empty-cell">
              {{ filteredOnly ? '没有被归因过滤的记录' : '暂无 CSV 数据' }}
            </td>
          </tr>
          <tr v-for="(row, idx) in items" :key="`${row.createdAt}-${row.model}-${idx}`" :class="{ filtered: row.filtered }">
            <td class="col-time">{{ formatTime(row.createdAt) }}</td>
            <td class="col-model" :title="row.model">{{ row.model }}</td>
            <td class="col-num" :title="String(row.input)">{{ formatNum(row.input) }}</td>
            <td class="col-num" :title="String(row.output)">{{ formatNum(row.output) }}</td>
            <td class="col-num" :title="String(row.cacheRead)">{{ formatNum(row.cacheRead) }}</td>
            <td class="col-num" :title="String(row.cacheCreation)">{{ formatNum(row.cacheCreation) }}</td>
            <td class="col-reason">
              <span v-if="row.reason" class="reason-tag" :class="row.reason">{{ reasonLabel(row.reason) }}</span>
              <span v-else class="reason-dash">—</span>
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
import { platformAdapter } from '@/platform'
import { formatNum } from '@/utils/format'
import type { CursorCsvPreviewRow, CursorFilterReason } from '@/platform/types'

const props = defineProps<{
  show: boolean
  initialFilteredOnly?: boolean
}>()

const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const PAGE_SIZE = 50
const filteredOnly = ref(false)
const page = ref(1)
const total = ref(0)
const items = ref<CursorCsvPreviewRow[]>([])
const loading = ref(false)

const totalPages = computed(() => Math.max(1, Math.ceil(total.value / PAGE_SIZE)))

function reasonLabel(reason: CursorFilterReason): string {
  if (reason === 'model') return '模型不对'
  if (reason === 'time') return '时间不对'
  return '无本机匹配'
}

function formatTime(epoch: number): string {
  return new Date(epoch * 1000).toLocaleString()
}

async function loadPage(): Promise<void> {
  loading.value = true
  try {
    const res = await platformAdapter.cursorPreviewCsv(page.value, PAGE_SIZE, filteredOnly.value)
    items.value = res.items
    total.value = res.total
    page.value = res.page
  } catch (e) {
    console.error('[cursor] preview csv failed:', e)
    items.value = []
    total.value = 0
  } finally {
    loading.value = false
  }
}

function onToggleFiltered(v: boolean): void {
  filteredOnly.value = v
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

.col-model {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col-num {
  width: 64px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.col-reason {
  width: 84px;
  text-align: center;
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
