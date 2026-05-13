import { computed, ref, watch, onUnmounted } from 'vue'

const INTERVALS = ['manual', '30s', '1min', '5min', '30min'] as const
type IntervalKey = (typeof INTERVALS)[number]

const INTERVAL_LABELS: Record<IntervalKey, string> = {
  manual: '手动',
  '30s': '30s',
  '1min': '1m',
  '5min': '5m',
  '30min': '30m'
}

const INTERVAL_TITLES: Record<IntervalKey, string> = {
  manual: '手动刷新（点击执行）',
  '30s': '每30秒自动刷新',
  '1min': '每1分钟自动刷新',
  '5min': '每5分钟自动刷新',
  '30min': '每30分钟自动刷新'
}

const INTERVAL_MS: Partial<Record<IntervalKey, number>> = {
  '30s': 30_000,
  '1min': 60_000,
  '5min': 300_000,
  '30min': 1_800_000
}

/**
 * 自动刷新 composable
 * @param onRefresh 每次定时触发时执行的回调
 */
export function useAutoRefresh(onRefresh: () => void) {
  const currentIndex = ref(2) // 默认 1min
  let timer: ReturnType<typeof setInterval> | null = null

  const intervalDisplay = computed(() => INTERVAL_LABELS[INTERVALS[currentIndex.value]])
  const intervalTitle = computed(() => INTERVAL_TITLES[INTERVALS[currentIndex.value]])

  function stopTimer(): void {
    if (timer) {
      clearInterval(timer)
      timer = null
    }
  }

  function startTimer(): void {
    stopTimer()
    const ms = INTERVAL_MS[INTERVALS[currentIndex.value]]
    if (ms) {
      timer = setInterval(onRefresh, ms)
    }
  }

  /** 点击间隔按钮：手动模式下立即刷新，然后切换到下一个间隔 */
  function cycleInterval(): void {
    if (INTERVALS[currentIndex.value] === 'manual') {
      onRefresh()
    }
    currentIndex.value = (currentIndex.value + 1) % INTERVALS.length
  }

  // 监听间隔变化，重启定时器
  watch(currentIndex, () => startTimer(), { immediate: true })

  onUnmounted(stopTimer)

  return {
    intervalDisplay,
    intervalTitle,
    cycleInterval
  }
}
