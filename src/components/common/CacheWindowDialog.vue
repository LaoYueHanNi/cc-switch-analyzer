<template>
  <Teleport to="body">
    <div v-if="show" class="cache-overlay" @click="$emit('update:show', false)" />
    <div
      v-if="show"
      class="cache-popup"
      ref="popupRef"
      :style="popupStyle"
    >
      <div class="cache-title">{{ modelName }} — 最近缓存窗口</div>

      <div v-if="loading" class="cache-loading">加载中...</div>

      <table v-else-if="windows.length > 0" class="cache-table">
        <thead>
          <tr>
            <th>开始</th>
            <th>结束</th>
            <th>时长</th>
            <th>命中</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(w, i) in windows" :key="i">
            <td>{{ w.startTime }}</td>
            <td>{{ w.endTime }}</td>
            <td class="dur">{{ w.duration }}</td>
            <td class="hits">{{ w.hits }}次</td>
          </tr>
        </tbody>
      </table>

      <div v-else class="empty-hint">暂无缓存数据</div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'
import { platformAdapter } from '@/platform'
import { epochToDateTimeStr, formatDuration } from '@/utils/format'

const props = defineProps<{
  show: boolean
  modelName: string
  modelId: string
  posX: number
  posY: number
}>()

defineEmits<{
  'update:show': [value: boolean]
}>()

interface CacheWindow {
  startTime: string
  endTime: string
  duration: string
  hits: number
}

const windows = ref<CacheWindow[]>([])
const loading = ref(false)
const popupRef = ref<HTMLElement | null>(null)
const popupStyle = ref<Record<string, string>>({ visibility: 'hidden' })

const POPUP_OFFSET = 4
const EDGE_MARGIN = 8

function reposition(): void {
  const el = popupRef.value
  if (!el) return
  const pw = el.offsetWidth
  const ph = el.offsetHeight
  const vw = window.innerWidth
  const vh = window.innerHeight

  let left = props.posX
  let top = props.posY + POPUP_OFFSET

  if (left + pw + EDGE_MARGIN > vw) {
    left = vw - pw - EDGE_MARGIN
  }
  if (top + ph + EDGE_MARGIN > vh) {
    top = props.posY - ph - POPUP_OFFSET
  }

  left = Math.max(EDGE_MARGIN, left)
  top = Math.max(EDGE_MARGIN, Math.min(top, vh - ph - EDGE_MARGIN))

  popupStyle.value = { left: left + 'px', top: top + 'px' }
}

async function loadCacheWindows(): Promise<void> {
  loading.value = true
  try {
    const result = await platformAdapter.queryCacheWindows(props.modelId)
    windows.value = (result || []).map((w: any) => ({
      startTime: epochToDateTimeStr(w.start_ts || w.startTs).split(' ')[1]
        ? epochToDateTimeStr(w.start_ts || w.startTs).replace(/^\d{2}\//, '')
        : '',
      endTime: epochToDateTimeStr(w.end_ts || w.endTs).split(' ')[1]
        ? epochToDateTimeStr(w.end_ts || w.endTs).replace(/^\d{2}\//, '')
        : '',
      duration: formatDuration(w.duration_sec || w.durationSec),
      hits: w.hits
    }))
  } catch (err) {
    console.error('缓存窗口加载失败:', err)
  } finally {
    loading.value = false
  }
}

watch(() => props.show, async (val) => {
  if (val && props.modelId) {
    popupStyle.value = { visibility: 'hidden' }
    await loadCacheWindows()
    await nextTick()
    reposition()
  }
})
</script>

<style scoped>
.cache-overlay {
  position: fixed;
  inset: 0;
  z-index: 999;
}

.cache-popup {
  position: fixed;
  z-index: 1000;
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 6px;
  padding: 10px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12);
  max-width: 400px;
}

.cache-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 6px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cache-loading {
  color: var(--text-muted);
  font-size: 12px;
  padding: 8px 0;
}

.cache-table {
  border-collapse: collapse;
  font-size: 11px;
}

.cache-table th {
  text-align: left;
  color: var(--text-muted);
  font-weight: 500;
  padding: 2px 8px 2px 0;
  border-bottom: 1px solid var(--border-faint);
  font-size: 10px;
}

.cache-table td {
  padding: 3px 8px 3px 0;
  color: var(--text-primary);
  border-bottom: 1px solid var(--bg-base);
  white-space: nowrap;
}

.dur {
  color: var(--color-green);
  font-weight: 500;
}

.hits {
  text-align: right;
}

.empty-hint {
  color: var(--text-muted);
  font-size: 12px;
  padding: 8px 0;
}
</style>
