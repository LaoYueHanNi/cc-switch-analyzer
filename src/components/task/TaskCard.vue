<template>
  <div class="task-card" @click="$emit('click')">
    <div class="card-header">
      <span class="task-title" :title="title">{{ title || '未命名任务' }}</span>
      <span
        class="status-tag"
        :class="`status-tag--${statusInfo.cls}`"
      >{{ statusInfo.label }}</span>
    </div>

    <div class="metrics-grid">
      <div v-if="totalCost > 0" class="metric">
        <span class="metric-value cost">{{ formatCost(totalCost) }}</span>
        <span class="metric-label">总费用</span>
      </div>
      <div class="metric">
        <span class="metric-value">{{ sessionCount }}</span>
        <span class="metric-label">个会话</span>
      </div>
      <div class="metric">
        <span class="metric-value small">{{ formatRelativeTime(updatedAt) }}</span>
        <span class="metric-label">最近活动</span>
      </div>
      <div v-if="totalTokens > 0" class="metric">
        <span class="metric-value green">{{ formatNum(totalTokens) }}</span>
        <span class="metric-label">Token</span>
      </div>
    </div>

    <div v-if="description" class="desc-row" :title="description">
      {{ description }}
    </div>

    <div class="footer" @click.stop="">
      <div class="action-group">
        <button
          class="open-all-btn"
          :disabled="sessionCount === 0"
          :title="sessionCount === 0 ? '暂无会话' : `一键打开全部 ${sessionCount} 个会话`"
          @click.stop="$emit('openAllSessions')"
        >▶ 一键打开</button>
        <div class="agent-launcher">
          <button
            class="agent-btn"
            @click.stop="$emit('launchAgent', 'claude')"
            @contextmenu.prevent="$emit('contextLaunchAgent', 'claude', $event)"
            title="新建 Claude Code（右键选择供应商配置）"
          >
            <span v-html="claudeSvg"></span>
          </button>
          <button class="agent-btn" @click.stop="$emit('launchAgent', 'opencode')" title="新建 OpenCode 会话">
            <span v-html="opencodeSvg"></span>
          </button>
          <button class="agent-btn" @click.stop="$emit('launchAgent', 'codex')" title="新建 Codex 会话">
            <span v-html="codexSvg"></span>
          </button>
        </div>
      </div>
      <div class="card-actions">
        <button class="action-btn" @click.stop="$emit('edit')" title="编辑">✎</button>
        <button class="action-btn danger" @click.stop="$emit('delete')" title="删除">×</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { formatNum, formatCost } from '@/utils/format'
import { TASK_STATUS_OPTIONS, type TaskStatus } from '@/types/task'
import claudeSvg from '@/assets/claude.svg?raw'
import opencodeSvg from '@/assets/opencode.svg?raw'
import codexSvg from '@/assets/codex.svg?raw'

const props = defineProps<{
  id: number
  title: string
  description: string
  status: TaskStatus
  createdAt: number
  updatedAt: number
  sessionCount: number
  totalTokens: number
  totalCost: number
}>()

defineEmits<{
  click: []
  edit: []
  delete: []
  launchAgent: [agent: 'claude' | 'opencode' | 'codex']
  contextLaunchAgent: [agent: 'claude' | 'opencode' | 'codex', event: MouseEvent]
  openAllSessions: []
}>()

const statusInfo = computed(() => {
  const o = TASK_STATUS_OPTIONS.find(x => x.value === props.status) ?? TASK_STATUS_OPTIONS[0]
  return { label: o.label, cls: o.value }
})

function formatRelativeTime(ts: number): string {
  if (!ts) return '-'
  const d = new Date(ts * 1000)
  const diffMin = Math.floor((Date.now() - d.getTime()) / 60000)
  if (diffMin < 1) return '刚刚'
  if (diffMin < 60) return `${diffMin}分钟前`
  const diffHours = Math.floor(diffMin / 60)
  if (diffHours < 24) return `${diffHours}小时前`
  const diffDays = Math.floor(diffHours / 24)
  if (diffDays === 1) return '昨天'
  if (diffDays < 7) return `${diffDays}天前`
  return d.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' })
}
</script>

<style scoped>
.task-card {
  background: var(--bg-card);
  border-radius: 6px;
  border: 1px solid var(--border-main);
  padding: 10px;
  min-width: 0;
  overflow: hidden;
  transition: box-shadow var(--transition-speed);
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.task-card:hover {
  box-shadow: var(--shadow-card);
}

.card-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 4px;
  min-width: 0;
}
.task-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}
.status-tag {
  flex-shrink: 0;
  font-size: 10px;
  padding: 0 6px;
  height: 16px;
  line-height: 16px;
  border-radius: 2px;
  background: var(--bg-hover);
  color: var(--text-muted);
  border: 1px solid var(--border-main);
}
.status-tag--todo { color: var(--text-muted); }
.status-tag--in_progress { color: var(--color-blue); border-color: var(--color-blue); }
.status-tag--done { color: var(--color-green); border-color: var(--color-green); }
.status-tag--archived { color: var(--text-faint); }

.metrics-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px 10px;
  padding: 4px 0 2px;
}
.metric {
  display: flex;
  align-items: baseline;
  gap: 4px;
  min-width: 0;
}
.metric-value {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.metric-value.cost {
  font-size: var(--font-size-cost);
  font-weight: 700;
  color: var(--color-cost);
}
.metric-value.green {
  color: var(--color-green);
}
.metric-value.small {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
}
.metric-label {
  font-size: 10px;
  color: var(--text-muted);
  white-space: nowrap;
}

.desc-row {
  font-size: 11px;
  color: var(--text-secondary);
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  margin-top: 2px;
  padding: 6px 8px;
  background: var(--bg-hover);
  border-radius: 3px;
}

.footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px solid var(--border-light);
}

.action-group {
  display: flex;
  align-items: center;
  gap: 4px;
}

.agent-launcher {
  display: flex;
  gap: 4px;
}
.agent-btn {
  border: none; background: none;
  cursor: pointer; padding: 2px 4px;
  display: flex; align-items: center; justify-content: center;
  color: var(--text-tertiary);
  transition: color var(--transition-speed);
  border-radius: 3px;
}
.agent-btn :deep(svg) {
  width: 14px; height: 14px;
}
.agent-btn:hover {
  color: var(--color-blue);
  background: var(--bg-hover);
}

.card-actions {
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity var(--transition-speed);
}
.task-card:hover .card-actions {
  opacity: 1;
}
.action-btn {
  border: none; background: none;
  cursor: pointer; padding: 0 4px;
  font-size: 12px;
  color: var(--text-tertiary);
  border-radius: 3px;
  line-height: 1;
  transition: all var(--transition-speed);
}
.action-btn:hover {
  color: var(--color-blue);
  background: var(--bg-hover);
}
.action-btn.danger:hover {
  color: var(--color-cost);
}

.open-all-btn {
  font-size: 10px;
  padding: 0 8px;
  height: 18px;
  line-height: 1;
  border: 1px solid var(--border-main);
  border-radius: 3px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  transition: all 0.15s;
}
.open-all-btn:hover:not(:disabled) {
  border-color: var(--color-blue);
  color: var(--color-blue);
}
.open-all-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
</style>
