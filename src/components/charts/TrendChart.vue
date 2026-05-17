<template>
  <div ref="chartRef" class="trend-chart" />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue'
import * as echarts from 'echarts'
import { useThemeStore } from '@/stores/theme'
import { formatNum, formatCost } from '@/utils/format'
import { getColor, hexToRgba } from '@/utils/color'

const props = defineProps<{
  dates: string[]
  totalCostData: number[]
  totalTokenData: number[]
  inputData: number[]
  outputData: number[]
  cacheReadData: number[]
  cacheCreationData: number[]
  visibleSeries: Record<string, boolean>
}>()

const themeStore = useThemeStore()
const chartRef = ref<HTMLElement>()
let chart: echarts.ECharts | null = null

function makeSeries(
  name: string,
  data: number[],
  colorVar: string,
  yAxisIndex: number,
  visible: boolean
): echarts.SeriesOption {
  const color = getColor(colorVar)
  return {
    name,
    type: 'line',
    data: visible ? data : data.map(() => null),
    yAxisIndex,
    smooth: true,
    showSymbol: false,
    connectNulls: false,
    lineStyle: { color, width: visible ? 2 : 0 },
    itemStyle: { color },
    areaStyle: visible ? {
      color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
        { offset: 0, color: hexToRgba(color, 0.25) },
        { offset: 1, color: hexToRgba(color, 0.02) }
      ])
    } : undefined,
    emphasis: { focus: 'series' },
    z: yAxisIndex === 0 ? 2 : 3
  }
}

function renderChart(): void {
  if (!chartRef.value) return
  if (!chart) chart = echarts.init(chartRef.value)

  const textMuted = getColor('--text-muted')
  const textPrimary = getColor('--text-primary')
  const borderColor = getColor('--border-main')

  const allTokenData = [
    ...props.totalTokenData, ...props.inputData,
    ...props.outputData, ...props.cacheReadData, ...props.cacheCreationData
  ]
  const tokenMax = Math.max(1, ...allTokenData)
  const costMax = Math.max(0.01, ...props.totalCostData)

  const series: echarts.SeriesOption[] = [
    makeSeries('总费用', props.totalCostData, '--color-cost', 1, props.visibleSeries.cost),
    makeSeries('总Token', props.totalTokenData, '--color-green', 0, props.visibleSeries.total),
    makeSeries('输入', props.inputData, '--color-purple', 0, props.visibleSeries.detail),
    makeSeries('输出', props.outputData, '--color-orange', 0, props.visibleSeries.detail),
    makeSeries('缓存读', props.cacheReadData, '--color-blue', 0, props.visibleSeries.detail),
    makeSeries('缓存写', props.cacheCreationData, '--color-dark-orange', 0, props.visibleSeries.detail),
  ]

  chart.setOption({
    grid: { top: 16, right: 60, bottom: 28, left: 60 },
    xAxis: {
      type: 'category',
      data: props.dates,
      axisLabel: { fontSize: 10, color: textMuted },
      axisTick: { show: false },
      axisLine: { lineStyle: { color: borderColor } }
    },
    yAxis: [
      {
        type: 'value',
        name: 'Token',
        min: 0,
        max: tokenMax,
        nameTextStyle: { fontSize: 10, color: textMuted },
        axisLabel: { show: true, fontSize: 10, color: textMuted, formatter: (v: number) => formatNum(v) },
        splitLine: { lineStyle: { color: borderColor, type: 'dashed' } }
      },
      {
        type: 'value',
        name: '费用',
        min: 0,
        max: costMax,
        nameTextStyle: { fontSize: 10, color: textMuted },
        axisLabel: {
          show: true, fontSize: 10, color: textMuted,
          formatter: (v: number) => '¥' + (v >= 1000 ? (v / 1000).toFixed(1) + 'K' : v.toFixed(2))
        },
        splitLine: { show: false }
      }
    ],
    tooltip: {
      trigger: 'axis',
      backgroundColor: themeStore.isDark ? 'rgba(34,34,64,0.95)' : 'rgba(255,255,255,0.95)',
      borderColor: borderColor,
      textStyle: { fontSize: 12, color: textPrimary },
      formatter: (params: any) => {
        if (!Array.isArray(params) || params.length === 0) return ''
        let html = `<div style="font-weight:600;margin-bottom:4px">${params[0].axisValue}</div>`
        for (const p of params) {
          if (p.value == null || p.value === '-') continue
          const marker = `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${p.color};margin-right:4px"></span>`
          const isCost = p.seriesName === '总费用'
          const val = isCost ? formatCost(p.value) : formatNum(p.value)
          html += `<div>${marker}${p.seriesName}: ${val}</div>`
        }
        return html
      }
    },
    series
  }, true)
}

onMounted(() => {
  requestAnimationFrame(() => {
    renderChart()
    window.addEventListener('resize', handleResize)
  })
})
watch(() => [
  props.dates, props.totalCostData, props.totalTokenData,
  props.inputData, props.outputData, props.cacheReadData,
  props.cacheCreationData, props.visibleSeries
], renderChart)
watch(() => themeStore.isDark, renderChart)

onUnmounted(() => {
  window.removeEventListener('resize', handleResize)
  if (chart) { chart.dispose(); chart = null }
})

function handleResize(): void { chart?.resize() }
</script>

<style scoped>
.trend-chart {
  width: 100%;
  height: 100%;
}
</style>
