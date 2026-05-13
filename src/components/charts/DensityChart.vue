<template>
  <div ref="chartRef" class="density-chart" />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue'
import * as echarts from 'echarts'
import { epochToTimeStr } from '@/utils/format'

const props = defineProps<{
  timestamps: number[]
  startTime: number
  endTime: number
}>()

const chartRef = ref<HTMLElement>()
let chart: echarts.ECharts | null = null

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
          { offset: 0, color: 'rgba(231,76,60,0.35)' },
          { offset: 1, color: 'rgba(231,76,60,0.02)' }
        ])
      }
    }],
    tooltip: {
      trigger: 'axis',
      formatter: () => `总请求数: ${props.timestamps.length}<br/>峰值: ${Math.max(...buckets)} 次`
    }
  }, true)
}

onMounted(renderChart)
watch(() => props.timestamps, renderChart, { deep: true })

onUnmounted(() => {
  if (chart) {
    chart.dispose()
    chart = null
  }
})
</script>

<style scoped>
.density-chart {
  width: 160px;
  height: 70px;
}
</style>
