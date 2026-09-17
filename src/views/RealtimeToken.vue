<template>
  <div class="realtime-token">
    <!-- 顶部统计 -->
    <div class="realtime-stats">
      <span class="stat-title">{{ statTitle }}</span>
      <div class="stat-items">
        <div class="realtime-stat">
          <span class="stat-label label-cost">总费用</span>
          <span class="stat-value cost">{{ formatCost(totalCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label label-green">总 Token</span>
          <span class="stat-value value-green">{{ formatNum(totalTokens) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label label-purple">输入费用</span>
          <span class="stat-value value-purple">{{ formatCost(totalInputCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label label-orange">输出费用</span>
          <span class="stat-value value-orange">{{ formatCost(totalOutputCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label label-blue">缓存读费用</span>
          <span class="stat-value value-blue">{{ formatCost(totalCacheReadCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label label-dark-orange">缓存写费用</span>
          <span class="stat-value value-dark-orange">{{ formatCost(totalCacheCreationCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label label-teal">缓存命中率</span>
          <span class="stat-value value-teal">{{ formatPercent(cacheHitRate) }}</span>
        </div>
      </div>
      <div class="live-badge" :class="{ 'live-pulse': hasNewData }">
        <span class="live-dot" />
        LIVE
      </div>
      <span class="refresh-time">{{ lastRefreshTime || '-' }}</span>
    </div>

    <!-- 工具栏:数据源过滤与翻页控制 -->
    <div v-if="logs.length > 0 || selectedSource" class="realtime-toolbar">
      <div class="toolbar-left">
        <span class="filter-label">数据源</span>
        <CompactSelect
          :model-value="selectedSource"
          :options="sourceOptions"
          clearable
          placeholder="全部"
          @update:model-value="selectedSource = $event"
        />
      </div>
      <div class="toolbar-right" v-if="filteredLogs.length > 0">
        <div class="realtime-pager">
          <span class="pager-info">第 {{ currentPage }} / {{ totalPages }} 页 (共 {{ filteredLogs.length }} 条)</span>
          <button class="pager-btn" :disabled="currentPage <= 1" @click="currentPage = 1">第一页</button>
          <button class="pager-btn" :disabled="currentPage <= 1" @click="currentPage--">上一页</button>
          <button class="pager-btn" :disabled="currentPage >= totalPages" @click="currentPage++">下一页</button>
        </div>
      </div>
    </div>

    <!-- 请求日志列表:平铺单表,每行最左侧标记数据源 -->
    <div v-if="filteredLogs.length > 0" class="log-list">
      <!-- 表头 -->
      <div class="log-header">
        <span class="col-source">数据源</span>
        <span class="col-time">时间</span>
        <span class="col-model">模型</span>
        <span class="col-token c-input">输入</span>
        <span class="col-token c-output">输出</span>
        <span class="col-token c-cache-r">缓存读</span>
        <span class="col-token c-cache-w">缓存写</span>
        <span class="col-total">总token</span>
        <span class="col-cost">费用</span>
        <span class="col-tier">档位</span>
        <span class="col-latency">首字</span>
        <span class="col-speed">输出速度</span>
      </div>
      <!-- 数据行 -->
      <div class="session-rows">
        <div
          class="log-row"
          :class="{ 'log-new': row.isNew }"
          v-for="(row, i) in pagedLogs"
          :key="`${row.createdAt}-${row.model}-${row.dbType}-${i}`"
        >
          <span class="col-source">
            <span class="source-dot" :class="dotClass(row.dbType)" />
            <span class="source-name">{{ row.dbType }}</span>
          </span>
          <span class="col-time">{{ formatTime(row.createdAt) }}</span>
          <span class="col-model" :title="row.model">{{ shortModel(row.model) }}</span>
          <span class="col-token c-input">
            <em>{{ formatNum(row.inputTokens) }}</em>
            <small>{{ formatCost(row.inputCost) }}</small>
          </span>
          <span class="col-token c-output">
            <em>{{ formatNum(row.outputTokens) }}</em>
            <small>{{ formatCost(row.outputCost) }}</small>
          </span>
          <span class="col-token c-cache-r">
            <em>{{ formatNum(row.cacheReadTokens) }}</em>
            <small>{{ formatCost(row.cacheReadCost) }}</small>
          </span>
          <span class="col-token c-cache-w">
            <em>{{ formatNum(row.cacheCreationTokens) }}</em>
            <small>{{ formatCost(row.cacheCreationCost) }}</small>
          </span>
          <span class="col-total">{{ formatNum(row.inputTokens + row.outputTokens + row.cacheReadTokens + row.cacheCreationTokens) }}</span>
          <span class="col-cost">{{ formatCost(row.totalCost) }}</span>
          <span class="col-tier" v-if="row.contextTierThreshold">>= {{ Math.round(row.contextTierThreshold / 1000) }}K</span>
          <span class="col-tier" v-else>-</span>
          <span class="col-latency">{{ formatLatency(row.latencyMs) }}</span>
          <span class="col-speed">{{ formatTokenSpeed(row.outputTokens, row.latencyMs) }}</span>
        </div>
      </div>
    </div>

    <!-- 筛选无匹配数据 -->
    <div v-else-if="logs.length > 0" class="realtime-empty">
      <p>无匹配该数据源的请求记录</p>
    </div>

    <!-- 全局无数据 -->
    <div v-else class="realtime-empty">
      <p>{{ dbStore.hasDatabase ? '暂无请求数据' : '请先选择数据库文件' }}</p>
    </div>
  </div>
</template>

<script setup lang="ts">
defineOptions({ name: 'RealtimeToken' })
import { ref, computed, onMounted, onActivated, onDeactivated, watch } from 'vue'
import CompactSelect from '@/components/common/CompactSelect.vue'
import { useDatabaseStore } from '@/stores/database'
import { useFilterStore } from '@/stores/filter'
import { useRealtimePolling } from '@/composables/useRealtimePolling'
import { formatNum, formatCost, formatPercent, formatTokenSpeed, formatLatency } from '@/utils/format'

const dbStore = useDatabaseStore()
const filterStore = useFilterStore()
const { logs, lastRefreshTime, startPolling, stopPolling, refreshNow } = useRealtimePolling()

// 数据源过滤与分页状态（独立于主页 FilterBar，避免改实时筛选带动模型/供应商查询）
const selectedSource = ref('')
const PAGE_SIZE = 50
const currentPage = ref(1)

// 数据源下拉：复用主页 FilterBar 的 providerOptions（canonical 名，如 CCS / OpenCode）
const sourceOptions = computed(() => [...filterStore.providerOptions])

// 数据源圆点配色(与设置页 slot key 命名一致)
function dotClass(dbType: string): string {
  const map: Record<string, string> = {
    'CCS': 'cc-switch', 'OpenCode': 'opencode', 'AIProxy': 'ai-proxy',
    'Cursor': 'cursor', 'ZCode': 'z-code', 'Proma': 'proma',
    'DSH': 'dsh', 'MiniMax': 'minimax', 'Antigravity': 'antigravity',
    'Kimi': 'kimi',
  }
  return map[dbType] ?? dbType.toLowerCase()
}

// 平铺列表:按时间倒序(后端已全局排序+截断 500,此处兜底再排)
const sortedLogs = computed(() => [...logs.value].sort((a, b) => b.createdAt - a.createdAt))

// 根据选择的数据源 canonical 名过滤（与主页下拉值一致，对应行的 dbType）
const filteredLogs = computed(() => {
  if (!selectedSource.value) return sortedLogs.value
  return sortedLogs.value.filter(r => r.dbType === selectedSource.value)
})

// 总页数
const totalPages = computed(() => Math.max(1, Math.ceil(filteredLogs.value.length / PAGE_SIZE)))

// 当前页数据切片（默认 50 条）
const pagedLogs = computed(() => {
  const start = (currentPage.value - 1) * PAGE_SIZE
  return filteredLogs.value.slice(start, start + PAGE_SIZE)
})

// 数据源切换时重置到第一页
watch(selectedSource, () => {
  currentPage.value = 1
})

// 当总页数变小时防止页码溢出
watch(totalPages, (pages) => {
  if (currentPage.value > pages) {
    currentPage.value = Math.max(1, pages)
  }
})

const hasNewData = computed(() => logs.value.some(r => r.isNew))

// 统计卡片标题
const statTitle = computed(() => {
  if (selectedSource.value) {
    const label = sourceOptions.value.find(o => o.value === selectedSource.value)?.label
      ?? selectedSource.value
    return `筛选: ${label} (${filteredLogs.value.length} 条)`
  }
  return logs.value.length > 0 ? `最近 ${logs.value.length} 条请求` : '最近 500 条请求'
})

// 统计数据：与当前过滤联动
const summaryStats = computed(() => {
  let cost = 0
  let tokens = 0
  let inputCost = 0
  let outputCost = 0
  let cacheReadCost = 0
  let cacheCreationCost = 0
  let inputTokens = 0
  let cacheReadTokens = 0
  let cacheCreationTokens = 0
  for (const r of filteredLogs.value) {
    cost += r.totalCost
    tokens += r.inputTokens + r.outputTokens + r.cacheReadTokens + r.cacheCreationTokens
    inputCost += r.inputCost
    outputCost += r.outputCost
    cacheReadCost += r.cacheReadCost
    cacheCreationCost += r.cacheCreationCost
    inputTokens += r.inputTokens
    cacheReadTokens += r.cacheReadTokens
    cacheCreationTokens += r.cacheCreationTokens
  }
  return {
    totalCost: cost,
    totalTokens: tokens,
    totalInputCost: inputCost,
    totalOutputCost: outputCost,
    totalCacheReadCost: cacheReadCost,
    totalCacheCreationCost: cacheCreationCost,
    cacheHitRate: (inputTokens + cacheReadTokens + cacheCreationTokens) > 0 ? cacheReadTokens / (inputTokens + cacheReadTokens + cacheCreationTokens) : 0
  }
})

const totalCost = computed(() => summaryStats.value.totalCost)
const totalTokens = computed(() => summaryStats.value.totalTokens)
const totalInputCost = computed(() => summaryStats.value.totalInputCost)
const totalOutputCost = computed(() => summaryStats.value.totalOutputCost)
const totalCacheReadCost = computed(() => summaryStats.value.totalCacheReadCost)
const totalCacheCreationCost = computed(() => summaryStats.value.totalCacheCreationCost)
const cacheHitRate = computed(() => summaryStats.value.cacheHitRate)

function formatTime(epoch: number): string {
  const d = new Date(epoch * 1000)
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  const hh = String(d.getHours()).padStart(2, '0')
  const mi = String(d.getMinutes()).padStart(2, '0')
  return `${mm}/${dd} ${hh}:${mi}`
}

function shortModel(name: string): string {
  if (name.length <= 24) return name
  return name.slice(0, 22) + '…'
}

onMounted(() => {
  if (dbStore.hasDatabase) startPolling()
})

onActivated(() => {
  if (dbStore.hasDatabase) startPolling(true)
})

onDeactivated(() => {
  stopPolling()
})

watch(() => dbStore.hasDatabase, (val) => {
  if (val) { refreshNow(); startPolling() } else { stopPolling() }
}, { immediate: true })
</script>

<style scoped>
.realtime-token {
  height: 100%;
  display: flex;
  flex-direction: column;
}

/* 顶部统计 */
.realtime-stats {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 4px 0 10px;
  flex-shrink: 0;
}
.stat-title { font-size: 13px; font-weight: 600; color: var(--text-secondary); }
.stat-items { display: flex; gap: 18px; }
.realtime-stat { display: flex; flex-direction: column; }
.stat-label { font-size: 10px; color: var(--text-faint); }
.stat-value { font-size: 15px; font-weight: 600; color: var(--text-primary); }
.stat-value.cost { color: var(--color-cost); }

.live-badge {
  display: flex; align-items: center; gap: 4px;
  font-size: 13px; font-weight: 700; color: var(--color-cost); margin-left: auto;
}
.live-dot {
  width: 7px; height: 7px; border-radius: 50%; background: var(--color-cost);
  animation: pulse 1.5s ease-in-out infinite;
}
@keyframes pulse { 0%,100%{opacity:1} 50%{opacity:.3} }
.live-pulse { animation: badge-pop 0.4s ease; }
@keyframes badge-pop { 0%{transform:scale(1)} 50%{transform:scale(1.2)} 100%{transform:scale(1)} }
.refresh-time { font-size: 11px; color: var(--text-faint); }

/* 日志列表 */
.log-list {
  flex: 1; min-height: 0; overflow-y: auto;
  background: transparent; border-radius: 0; border: 0; font-size: 12px;
}

.session-rows {
  content-visibility: auto;
}

.log-header {
  display: flex; align-items: center; padding: 4px 10px;
  font-size: 10px; font-weight: 600; text-transform: uppercase; letter-spacing: .3px;
  border-bottom: 1px solid var(--border-faint);
  background: var(--bg-base);
  position: sticky; top: 0; z-index: 1;
}

.log-row {
  display: flex; align-items: center; padding: 4px 10px;
  border-bottom: 1px solid var(--border-faint);
  content-visibility: auto;
}
.log-row:nth-child(even) { background: var(--bg-card-alt); }

/* 新行高亮动画 */
.log-new {
  animation: row-flash 1.5s ease-out;
}
@keyframes row-flash {
  0% { background: var(--bg-flash); }
  100% { background: transparent; }
}

/* 列宽 - 数据源 */
.col-source {
  width: 92px; flex-shrink: 0;
  display: flex; align-items: center; gap: 5px;
  color: var(--text-primary);
}
.col-source .source-name {
  font-size: 11px; font-weight: 600; color: var(--text-primary);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

/* 数据源圆点配色 */
.source-dot {
  width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0;
}
.source-dot.cc-switch { background: var(--color-blue); }
.source-dot.opencode { background: var(--color-amber); }
.source-dot.ai-proxy { background: var(--color-green); }
.source-dot.z-code { background: var(--text-secondary); }
.source-dot.proma { background: var(--color-orange); }
.source-dot.dsh { background: var(--color-green); }
.source-dot.minimax { background: var(--text-muted); }
.source-dot.antigravity { background: var(--color-purple, var(--color-indigo)); }
.source-dot.cursor { background: var(--color-indigo); }

/* 列宽 - 基础 */
.col-time { width: 80px; flex-shrink: 0; color: var(--text-primary); font-weight: 500; }
.col-model { width: 150px; flex-shrink: 0; font-weight: 500; color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

/* Token 列 - 带配色 */
.col-token {
  width: 76px; flex-shrink: 0;
  display: flex; flex-direction: column; align-items: flex-end;
}
.col-token em { font-style: normal; font-size: 12px; font-weight: 500; }
.col-token small { font-size: 9px; color: var(--text-faint); line-height: 1; }

/* 表头配色 */
.log-header .c-input { color: var(--color-purple); }
.log-header .c-output { color: var(--color-orange); }
.log-header .c-cache-r { color: var(--color-blue); }
.log-header .c-cache-w { color: var(--color-dark-orange); }

/* 总 token 列不转大写 */
.log-header .col-total { text-transform: none; letter-spacing: 0; }

/* 数据行 Token 配色 */
.c-input em { color: var(--color-purple); }
.c-output em { color: var(--color-orange); }
.c-cache-r em { color: var(--color-blue); }
.c-cache-w em { color: var(--color-dark-orange); }

/* 总 Token 列 */
.col-total { width: 64px; flex-shrink: 0; text-align: right; font-weight: 600; color: var(--color-green); font-size: 12px; }

/* 费用列 */
.col-cost { width: 68px; flex-shrink: 0; text-align: right; font-weight: 700; color: var(--color-cost); font-size: 12px; }

/* 档位列 */
.col-tier { width: 48px; flex-shrink: 0; text-align: right; font-size: 10px; color: var(--text-secondary); }

/* 首字列（展示 latencyMs） */
.col-latency { width: 50px; flex-shrink: 0; text-align: right; color: var(--text-muted); font-size: 11px; }

/* 速度列 */
.col-speed { width: 90px; flex-shrink: 0; text-align: right; color: var(--color-orange); font-size: 11px; }
.log-header .col-speed { color: var(--color-orange); }

/* 工具栏:数据源过滤与翻页控制 */
.realtime-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 10px 8px;
  flex-shrink: 0;
  border-bottom: 1px solid var(--border-faint);
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 3px;
}

.filter-label {
  font-size: 11px;
  color: var(--text-tertiary);
  white-space: nowrap;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 6px;
}

/* 分页控件 */
.realtime-pager {
  display: flex;
  align-items: center;
  gap: 6px;
}

.pager-info {
  font-size: 11px;
  color: var(--text-muted);
}

.pager-btn {
  font-size: 11px;
  padding: 2px 8px;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: var(--bg-card);
  color: var(--text-primary);
  cursor: pointer;
  transition: border-color 0.15s, color 0.15s;
}

.pager-btn:hover:not(:disabled) {
  border-color: var(--color-blue);
  color: var(--color-blue);
}

.pager-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.realtime-empty {
  flex: 1; display: flex; align-items: center; justify-content: center; color: var(--text-muted);
}

/* 彩色统计标签 */
.label-purple, .value-purple { color: var(--color-purple); }
.label-orange, .value-orange { color: var(--color-orange); }
.label-blue, .value-blue { color: var(--color-blue); }
.label-dark-orange, .value-dark-orange { color: var(--color-dark-orange); }
.label-teal, .value-teal { color: var(--color-teal); }
.label-green, .value-green { color: var(--color-green); }
.label-cost { color: var(--color-cost); }
</style>