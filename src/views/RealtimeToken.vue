<template>
  <div class="realtime-token">
    <!-- 顶部统计 -->
    <div class="realtime-stats">
      <span class="stat-title">最近 500 条请求</span>
      <div class="stat-items">
        <div class="realtime-stat">
          <span class="stat-label">总费用</span>
          <span class="stat-value cost">{{ formatCost(totalCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label">总 Token</span>
          <span class="stat-value">{{ formatNum(totalTokens) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label" style="color: var(--color-purple)">输入费用</span>
          <span class="stat-value" style="color: var(--color-purple)">{{ formatCost(totalInputCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label" style="color: var(--color-orange)">输出费用</span>
          <span class="stat-value" style="color: var(--color-orange)">{{ formatCost(totalOutputCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label" style="color: var(--color-blue)">缓存读费用</span>
          <span class="stat-value" style="color: var(--color-blue)">{{ formatCost(totalCacheReadCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label" style="color: var(--color-dark-orange)">缓存写费用</span>
          <span class="stat-value" style="color: var(--color-dark-orange)">{{ formatCost(totalCacheCreationCost) }}</span>
        </div>
        <div class="realtime-stat">
          <span class="stat-label">缓存命中率</span>
          <span class="stat-value">{{ formatPercent(cacheHitRate) }}</span>
        </div>
      </div>
      <div class="live-badge" :class="{ 'live-pulse': hasNewData }">
        <span class="live-dot" />
        LIVE
      </div>
      <span class="refresh-time">{{ lastRefreshTime || '-' }}</span>
    </div>

    <!-- 请求日志列表 -->
    <div v-if="logs.length > 0" class="log-list">
      <template v-for="(group, gi) in sessionGroups" :key="group.sessionId">
        <!-- Session 分组头 -->
        <div class="session-header" :class="{ 'session-new': groupHasNew(group) }" @click="toggleGroup(group.sessionId)">
          <span class="sh-arrow" :class="{ collapsed: collapsedSessions.has(group.sessionId) }">▾</span>
          <span class="sh-id">{{ shortSession(group.sessionId) }}</span>
          <span class="sh-count">{{ group.rows.length }} 次</span>
          <span class="sh-cost">{{ formatCost(group.cost) }}</span>
          <span class="sh-time">{{ formatTime(group.rows[0].createdAt) }} ~ {{ formatTime(group.rows[group.rows.length - 1].createdAt).split(' ').pop() }}</span>
        </div>
        <template v-if="!collapsedSessions.has(group.sessionId)">
        <div class="session-body">
        <!-- 表头 -->
        <div class="log-header">
          <span class="col-time">时间</span>
          <span class="col-model">模型</span>
          <span class="col-token c-input">输入</span>
          <span class="col-token c-output">输出</span>
          <span class="col-token c-cache-r">缓存读</span>
          <span class="col-token c-cache-w">缓存写</span>
          <span class="col-total">总token</span>
          <span class="col-cost">费用</span>
          <span class="col-latency">延迟</span>
        </div>
        <!-- 数据行 -->
        <div class="session-rows">
        <div class="log-row" :class="{ 'log-new': (row as any)._new }" v-for="(row, ri) in group.rows" :key="row.createdAt + row.model + ri">
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
          <span class="col-latency">{{ formatLatency(row.latencyMs) }}</span>
        </div>
        </div>
        </div>
        </template>
      </template>
    </div>

    <div v-else class="realtime-empty">
      <p>{{ dbStore.hasDatabase ? '暂无请求数据' : '请先选择数据库文件' }}</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, onMounted, onBeforeUnmount, watch } from 'vue'
import { useDatabaseStore } from '@/stores/database'
import { useRealtimePolling } from '@/composables/useRealtimePolling'
import { formatNum, formatCost, formatPercent } from '@/utils/format'
import type { RealtimeRequestLog } from '@/types/database'

const dbStore = useDatabaseStore()
const { logs, lastRefreshTime, startPolling, stopPolling, refreshNow } = useRealtimePolling()

const collapsedSessions = ref<Set<string>>(new Set())

function toggleGroup(sessionId: string): void {
  const next = new Set(collapsedSessions.value)
  if (next.has(sessionId)) next.delete(sessionId)
  else next.add(sessionId)
  collapsedSessions.value = next
}

function groupHasNew(group: SessionGroup): boolean {
  return group.rows.some(r => (r as any)._new)
}

const hasNewData = computed(() => logs.value.some(r => (r as any)._new))

const totalCost = computed(() =>
  logs.value.reduce((sum, r) => sum + r.totalCost, 0)
)

const totalTokens = computed(() =>
  logs.value.reduce((sum, r) =>
    sum + r.inputTokens + r.outputTokens + r.cacheReadTokens + r.cacheCreationTokens, 0
  )
)

const totalInputCost = computed(() => logs.value.reduce((s, r) => s + r.inputCost, 0))
const totalOutputCost = computed(() => logs.value.reduce((s, r) => s + r.outputCost, 0))
const totalCacheReadCost = computed(() => logs.value.reduce((s, r) => s + r.cacheReadCost, 0))
const totalCacheCreationCost = computed(() => logs.value.reduce((s, r) => s + r.cacheCreationCost, 0))
const cacheHitRate = computed(() => {
  const input = logs.value.reduce((s, r) => s + r.inputTokens, 0)
  const cacheRead = logs.value.reduce((s, r) => s + r.cacheReadTokens, 0)
  return (input + cacheRead) > 0 ? cacheRead / (input + cacheRead) : 0
})

interface SessionGroup {
  sessionId: string
  rows: RealtimeRequestLog[]
  cost: number
}

const sessionGroups = computed<SessionGroup[]>(() => {
  const map = new Map<string, RealtimeRequestLog[]>()
  for (const log of logs.value) {
    const sid = log.sessionId || 'unknown'
    if (!map.has(sid)) map.set(sid, [])
    map.get(sid)!.push(log)
  }
  // 每组按时间倒序
  const groups: SessionGroup[] = []
  for (const [sessionId, rows] of map) {
    rows.sort((a, b) => b.createdAt - a.createdAt)
    groups.push({
      sessionId,
      rows,
      cost: rows.reduce((s, r) => s + r.totalCost, 0)
    })
  }
  // 按组内最新时间倒序
  groups.sort((a, b) => b.rows[0].createdAt - a.rows[0].createdAt)
  return groups
})

function formatTime(epoch: number): string {
  const d = new Date(epoch * 1000)
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  const hh = String(d.getHours()).padStart(2, '0')
  const mi = String(d.getMinutes()).padStart(2, '0')
  return `${mm}/${dd} ${hh}:${mi}`
}

function shortSession(sessionId: string): string {
  const parts = sessionId.split('-')
  return parts[0] || sessionId.slice(0, 8)
}

function shortModel(name: string): string {
  if (name.length <= 24) return name
  return name.slice(0, 22) + '...'
}

function formatLatency(ms: number): string {
  if (ms >= 1000) return (ms / 1000).toFixed(1) + 's'
  return ms + 'ms'
}

onMounted(() => {
  if (dbStore.hasDatabase) startPolling()
})

onBeforeUnmount(() => {
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
.stat-value.cost { color: var(--color-amber); }

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
  background: var(--bg-card); border-radius: 6px; border: 1px solid var(--border-main); font-size: 12px;
}

/* Session 分组头 */
.session-header {
  display: flex; align-items: center; gap: 8px;
  padding: 5px 10px;
  background: var(--bg-card-alt);
  border-bottom: 1px solid var(--border-main);
  border-top: 1px solid var(--border-main);
  position: sticky; top: 0; z-index: 2;
  cursor: pointer;
  user-select: none;
}
.session-header:hover { background: var(--bg-hover); }
.session-new { animation: row-flash 1.5s ease-out; }
.sh-arrow {
  font-size: 10px; color: var(--text-muted); transition: transform 0.15s;
}
.sh-arrow.collapsed { transform: rotate(-90deg); }
.sh-id { font-size: 12px; font-weight: 700; color: var(--color-blue); }
.sh-count { font-size: 10px; color: var(--text-muted); }
.sh-cost { font-size: 11px; font-weight: 600; color: var(--color-cost); }
.sh-time { font-size: 10px; color: var(--text-faint); margin-left: auto; }

.session-body {
  border-bottom: 1px solid var(--border-main);
}

.session-rows {
  max-height: 270px;
  overflow-y: auto;
}

.log-header {
  display: flex; align-items: center; padding: 4px 10px;
  font-size: 10px; font-weight: 600; text-transform: uppercase; letter-spacing: .3px;
  border-bottom: 1px solid var(--border-faint);
  background: var(--bg-card);
  position: sticky; top: 0; z-index: 1;
}

.log-row {
  display: flex; align-items: center; padding: 4px 10px;
  border-bottom: 1px solid var(--border-faint);
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
.col-total { width: 64px; flex-shrink: 0; text-align: right; font-weight: 600; color: var(--text-primary); font-size: 12px; }

/* 费用列 */
.col-cost { width: 68px; flex-shrink: 0; text-align: right; font-weight: 700; color: var(--color-amber); font-size: 12px; }

/* 延迟列 */
.col-latency { width: 50px; flex-shrink: 0; text-align: right; color: var(--text-muted); font-size: 11px; }

.realtime-empty {
  flex: 1; display: flex; align-items: center; justify-content: center; color: var(--text-muted);
}
</style>
