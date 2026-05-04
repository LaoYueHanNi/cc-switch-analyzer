<template>
  <div class="realtime-chart-wrapper">
    <div ref="chartRef" class="realtime-chart" />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, watch, onBeforeUnmount } from 'vue'
import * as echarts from 'echarts'
import type { RealtimeBucket } from '@/types/database'

const props = defineProps<{
  buckets: RealtimeBucket[]
}>()

const chartRef = ref<HTMLElement>()
let chart: echarts.ECharts | null = null

function renderChart(): void {
  if (!chartRef.value) return
  if (!chart) chart = echarts.init(chartRef.value)

  const now = Math.floor(Date.now() / 1000)
  const oneHourAgo = now - 3600

  // 构建完整的时间桶（360 个 10 秒桶）
  const bucketMap = new Map<number, number>()
  for (const b of props.buckets) {
    const total = (b.inputTokens || 0) + (b.outputTokens || 0) + (b.cacheRead || 0) + (b.cacheCreation || 0)
    bucketMap.set(b.bucket, total)
  }

  const timeLabels: string[] = []
  const data: number[] = []

  for (let t = oneHourAgo; t <= now; t += 10) {
    const bucket = Math.floor(t / 10) * 10
    if (t % 60 === 0) {
      const d = new Date(t * 1000)
      timeLabels.push(d.getHours().toString().padStart(2, '0') + ':' + d.getMinutes().toString().padStart(2, '0'))
    } else {
      timeLabels.push('')
    }
    data.push(bucketMap.get(bucket) || 0)
  }

  chart.setOption({
    grid: { top: 30, right: 16, bottom: 24, left: 50 },
    xAxis: {
      type: 'category',
      data: timeLabels,
      axisLabel: { fontSize: 10, color: '#999', interval: Math.floor(timeLabels.length / 6) },
      axisTick: { show: false }
    },
    yAxis: {
      type: 'value',
      axisLabel: {
        fontSize: 10,
        color: '#999',
        formatter: (v: number) => v >= 1000000 ? (v / 1_000_000).toFixed(1) + 'M' : v >= 1000 ? (v / 1000).toFixed(0) + 'K' : String(v)
      }
    },
    series: [{
      type: 'line',
      data,
      smooth: true,
      showSymbol: false,
      lineStyle: { color: '#4a90d9', width: 2 },
      areaStyle: {
        color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
          { offset: 0, color: 'rgba(74,144,217,0.3)' },
          { offset: 1, color: 'rgba(74,144,217,0.02)' }
        ])
      }
    }],
    animation: false
  }, true)
}

onMounted(renderChart)
watch(() => props.buckets, renderChart)

function resize(): void { chart?.resize() }
window.addEventListener('resize', resize)
onBeforeUnmount(() => {
  window.removeEventListener('resize', resize)
  chart?.dispose()
})
</script>

<style scoped>
.realtime-chart-wrapper {
  position: relative;
  flex: 1;
  min-height: 0;
}

.realtime-chart {
  width: 100%;
  height: 100%;
}
</style>
