<template>
  <div ref="chartRef" class="density-chart" />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue'
import * as echarts from 'echarts'
import { epochToTimeStr } from '@/utils/format'
import { useThemeStore } from '@/stores/theme'

const props = defineProps<{
  timestamps: number[]
  startTime: number
  endTime: number
}>()

const themeStore = useThemeStore()
const chartRef = ref<HTMLElement>()
let chart: echarts.ECharts | null = null

function hexToRgba(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return `rgba(${r},${g},${b},${alpha})`
}

function renderChart(): void {
  if (!chartRef.value || props.timestamps.length === 0) return

  if (!chart) {
    chart = echarts.init(chartRef.value)
  }

  const cs = getComputedStyle(document.documentElement)
  const textMuted = cs.getPropertyValue('--text-muted').trim()
  const colorCost = cs.getPropertyValue('--color-cost').trim()

  const duration = props.endTime - props.startTime
  if (duration <= 0) return

  // 分 32 个时间桶
  const bucketCount = 32
  const bucketSize = duration / bucketCount
  const buckets: number[] = new Array(bucketCount).fill(0)

  for (const ts of props.timestamps) {
    const offset = ts - props.startTime
    const idx = Math.min(Math.floor(offset / bucketSize), bucketCount - 1)
    if (idx >= 0) buckets[idx]++
  }

  const startLabel = epochToTimeStr(props.startTime)
  const endLabel = epochToTimeStr(props.endTime)

  chart.setOption({
    grid: { top: 4, right: 10, bottom: 18, left: 10 },
    xAxis: {
      show: true,
      type: 'category',
      data: [startLabel, ...new Array(bucketCount - 2).fill(''), endLabel],
      axisLabel: { fontSize: 9, color: textMuted, interval: 0 },
      axisTick: { show: false },
      axisLine: { show: false }
    },
    yAxis: { show: false, min: 0 },
    series: [{
      type: 'line',
      data: buckets,
      smooth: true,
      showSymbol: false,
      lineStyle: { color: colorCost, width: 1.5 },
      areaStyle: {
        color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
          { offset: 0, color: hexToRgba(colorCost, 0.35) },
          { offset: 1, color: hexToRgba(colorCost, 0.02) }
        ])
      }
    }],
    tooltip: {
      trigger: 'axis',
      formatter: () => `总请求数: ${props.timestamps.length}<br/>峰值: ${Math.max(...buckets)} 次`
    }
  }, true)
}

onMounted(() => {
  renderChart()
  window.addEventListener('resize', handleResize)
})
watch(() => props.timestamps, renderChart, { deep: true })
watch(() => themeStore.isDark, renderChart)

onUnmounted(() => {
  window.removeEventListener('resize', handleResize)
  if (chart) {
    chart.dispose()
    chart = null
  }
})

function handleResize(): void {
  chart?.resize()
}
</script>

<style scoped>
.density-chart {
  width: 160px;
  height: 70px;
}
</style>
