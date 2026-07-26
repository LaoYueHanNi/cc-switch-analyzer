<template>
  <div class="project-card" @click="$emit('click')">
    <div class="card-header">
      <span class="project-name" :title="projectDir">{{ displayName }}</span>
      <div v-if="projectDir" class="terminal-btns" :class="{ active: terminalActive }">
        <button class="terminal-btn" @click.stop="$emit('terminal', projectDir)" @contextmenu.prevent="$emit('contextTerminal', projectDir, $event)" title="Claude Code（右键选择供应商配置）">
          <span v-html="claudeSvg"></span>
        </button>
        <button class="terminal-btn" @click.stop="$emit('openCodeTerminal', projectDir)" title="OpenCode">
          <span v-html="opencodeSvg"></span>
        </button>
        <button class="terminal-btn" @click.stop="$emit('codexTerminal', projectDir)" title="Codex">
          <span v-html="codexSvg"></span>
        </button>
        <button class="terminal-btn" @click.stop="$emit('grokTerminal', projectDir)" title="Grok Build">
          <span v-html="grokSvg"></span>
        </button>
      </div>
    </div>

    <div v-if="totalCost > 0" class="cost-section">
      <span class="cost-value">{{ formatCost(totalCost) }}</span>
      <span class="cost-label">总费用</span>
    </div>

    <div class="info-section">
      <span class="info-value">{{ sessionCount }}</span>
      <span class="info-label">个会话</span>
    </div>

    <div class="stats-row">
      <span class="stat-item">{{ formatTime(lastActiveAt) }}</span>
      <span v-if="totalTokens > 0" class="stat-item">{{ formatNum(totalTokens) }} Token</span>
    </div>

    <div class="path-row" @click.stop="">
      <span class="path-text" :title="projectDir">{{ projectDir }}</span>
      <button class="copy-btn" @click.stop="copyPath" title="复制路径">{{ copied ? '✓' : '⧉' }}</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { formatNum, formatCost } from '@/utils/format'
import claudeSvg from '@/assets/claude.svg?raw'
import opencodeSvg from '@/assets/opencode.svg?raw'
import codexSvg from '@/assets/codex.svg?raw'
import grokSvg from '@/assets/grok.svg?raw'

const props = defineProps<{
  displayName: string
  projectDir: string
  sessionCount: number
  lastActiveAt: number
  totalCost: number
  totalTokens: number
  terminalActive?: boolean
}>()

defineEmits<{ click: []; terminal: [dir: string]; contextTerminal: [dir: string, event: MouseEvent]; openCodeTerminal: [dir: string]; codexTerminal: [dir: string]; grokTerminal: [dir: string] }>()

const copied = ref(false)

async function copyPath() {
  try {
    await navigator.clipboard.writeText(props.projectDir)
    copied.value = true
    setTimeout(() => { copied.value = false }, 1500)
  } catch {}
}

function formatTime(ts: number): string {
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
.project-card {
  background: var(--bg-card);
  border-radius: 6px;
  border: 1px solid var(--border-main);
  padding: 10px;
  min-width: 0;
  overflow: hidden;
  transition: box-shadow var(--transition-speed);
  cursor: pointer;
}
.project-card:hover {
  box-shadow: var(--shadow-card);
}

.card-header {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-bottom: 4px;
  min-width: 0;
}
.project-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.terminal-btns {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity var(--transition-speed);
}
.project-card:hover .terminal-btns,
.terminal-btns.active {
  opacity: 1;
}
.terminal-btn {
  border: none; background: none;
  cursor: pointer; padding: 0 2px;
  display: flex; align-items: center; justify-content: center;
  color: var(--text-tertiary);
  transition: color var(--transition-speed);
}
.terminal-btn :deep(svg) {
  width: 14px; height: 14px;
}
.terminal-btn:hover {
  color: var(--color-blue);
}

.cost-section {
  display: flex;
  align-items: baseline;
  gap: 4px;
  margin-bottom: 2px;
}
.cost-value {
  font-size: var(--font-size-cost);
  font-weight: 700;
  color: var(--color-cost);
}
.cost-label {
  font-size: 10px;
  color: var(--text-muted);
}

.info-section {
  display: flex;
  align-items: baseline;
  gap: 4px;
  margin-bottom: 4px;
}
.info-value {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-green);
}
.info-label {
  font-size: 10px;
  color: var(--text-muted);
}

.stats-row {
  display: flex;
  gap: 8px;
  margin-bottom: 4px;
  font-size: 10px;
}
.stat-item {
  color: var(--text-tertiary);
  white-space: nowrap;
}

.path-row {
  display: flex; align-items: center; gap: 4px;
  font-size: 10px; color: var(--text-faint);
}
.path-text {
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1;
}
.copy-btn {
  border: none; background: none; color: var(--text-faint);
  font-size: 10px; cursor: pointer; padding: 0 1px; flex-shrink: 0;
  line-height: 1; transition: color var(--transition-speed);
}
.copy-btn:hover { color: var(--color-blue); }
</style>
