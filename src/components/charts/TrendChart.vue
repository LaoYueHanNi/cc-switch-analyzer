<template>
  <div ref="chartRef" class="trend-chart" />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue'
import * as echarts from 'echarts'
import { useThemeStore } from '@/stores/theme'
import { formatNum, formatCost } from '@/utils/format'
import { getColor, hexToRgba } from '@/utils/color'

export interface ModelSeries {
  name: string
  data: number[]
  colorVar: string
  visible: boolean
}

const props = withDefaults(defineProps<{
  dates: string[]
  totalCostData: number[]
  totalTokenData: number[]
  inputData: number[]
  outputData: number[]
  cacheReadData: number[]
  cacheCreationData: number[]
  visibleSeries: Record<string, boolean>
  mode?: 'overview' | 'byModel'
  modelSeries?: ModelSeries[]
  dimmedModels?: string[]
}>(), {
  mode: 'overview',
  modelSeries: () => [],
  dimmedModels: () => []
})

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
  const pointCount = data.filter(v => v != null && v > 0).length
  const baseSize = pointCount <= 7 ? 6 : pointCount <= 31 ? 4 : 3

  return {
    name,
    type: 'line',
    data: visible ? data : data.map(() => null),
    yAxisIndex,
    smooth: true,
    showSymbol: true,
    symbol: 'circle',
    symbolSize: (value: number) => value > 0 ? baseSize : 0,
    connectNulls: false,
    lineStyle: { color, width: visible ? 2 : 0 },
    itemStyle: { color },
    areaStyle: visible ? {
      color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
        { offset: 0, color: hexToRgba(color, 0.25) },
        { offset: 1, color: hexToRgba(color, 0.02) }
      ])
    } : undefined,
    emphasis: {
      focus: 'series',
      scale: true,
      symbolSize: baseSize * 2,
      itemStyle: {
        borderWidth: 2,
        borderColor: '#fff',
        shadowBlur: 6,
        shadowColor: 'rgba(0,0,0,0.2)'
      }
    },
    z: yAxisIndex === 0 ? 2 : 3
  }
}

function makeModelSeries(s: ModelSeries): echarts.SeriesOption {
  const color = getColor(s.colorVar)
  const pointCount = s.data.filter(v => v != null && v > 0).length
  const baseSize = pointCount <= 7 ? 6 : pointCount <= 31 ? 4 : 3
  const visible = s.visible
  const dimmed = props.dimmedModels.includes(s.name)
  const opacity = dimmed ? 0.15 : 1

  return {
    name: s.name,
    type: 'line',
    data: visible ? s.data : s.data.map(() => null),
    yAxisIndex: 0,
    smooth: true,
    showSymbol: true,
    symbol: 'circle',
    symbolSize: (value: number) => value > 0 ? baseSize : 0,
    connectNulls: false,
    lineStyle: { color, width: visible ? 2 : 0, opacity },
    itemStyle: { color, opacity },
    emphasis: {
      focus: 'series',
      scale: true,
      symbolSize: baseSize * 2,
      lineStyle: { opacity: 1 },
      itemStyle: {
        borderWidth: 2,
        borderColor: '#fff',
        shadowBlur: 6,
        shadowColor: 'rgba(0,0,0,0.2)'
      }
    },
    z: 2
  }
}

function renderChart(): void {
  if (!chartRef.value) return
  if (!chart) chart = echarts.init(chartRef.value)

  const textMuted = getColor('--text-muted')
  const textPrimary = getColor('--text-primary')
  const borderColor = getColor('--border-main')
  const isByModel = props.mode === 'byModel'

  let series: echarts.SeriesOption[]
  let tokenArrays: number[][]
  let costMax: number
  let yAxisOptions: any[]

  if (isByModel) {
    const modelDataArrays = props.modelSeries
      .filter(s => s.visible)
      .map(s => s.data)
    tokenArrays = modelDataArrays
    costMax = 0.01
    series = props.modelSeries.map(s => makeModelSeries(s))
    const tokenMax = tokenArrays.length > 0 ? Math.max(1, ...tokenArrays.flat()) : 1
    yAxisOptions = [{
      type: 'value',
      name: 'Token',
      min: 0,
      max: tokenMax,
      nameTextStyle: { fontSize: 10, color: textMuted },
      axisLabel: { fontSize: 10, color: textMuted, formatter: (v: number) => formatNum(v) },
      splitLine: { lineStyle: { color: borderColor, type: 'dashed' } }
    }]
  } else {
    tokenArrays = []
    if (props.visibleSeries.total) tokenArrays.push(props.totalTokenData)
    if (props.visibleSeries.detail) tokenArrays.push(props.inputData, props.outputData, props.cacheReadData, props.cacheCreationData)
    const tokenMax = tokenArrays.length > 0 ? Math.max(1, ...tokenArrays.flat()) : 1
    costMax = props.visibleSeries.cost ? Math.max(0.01, ...props.totalCostData) : 0.01
    series = [
      makeSeries('总费用', props.totalCostData, '--color-cost', 1, props.visibleSeries.cost),
      makeSeries('总Token', props.totalTokenData, '--color-green', 0, props.visibleSeries.total),
      makeSeries('输入', props.inputData, '--color-purple', 0, props.visibleSeries.detail),
      makeSeries('输出', props.outputData, '--color-orange', 0, props.visibleSeries.detail),
      makeSeries('缓存读', props.cacheReadData, '--color-blue', 0, props.visibleSeries.detail),
      makeSeries('缓存写', props.cacheCreationData, '--color-dark-orange', 0, props.visibleSeries.detail),
    ]
    yAxisOptions = [
      {
        type: 'value',
        name: 'Token',
        min: 0,
        max: tokenMax,
        nameTextStyle: { fontSize: 10, color: textMuted, opacity: tokenArrays.length > 0 ? 1 : 0 },
        axisLabel: { show: tokenArrays.length > 0, fontSize: 10, color: textMuted, formatter: (v: number) => formatNum(v) },
        splitLine: { lineStyle: { color: borderColor, type: 'dashed' } }
      },
      {
        type: 'value',
        name: '费用',
        min: 0,
        max: costMax,
        nameTextStyle: { fontSize: 10, color: textMuted, opacity: props.visibleSeries.cost ? 1 : 0 },
        axisLabel: {
          show: props.visibleSeries.cost, fontSize: 10, color: textMuted,
          formatter: (v: number) => '¥' + (v >= 1000 ? (v / 1000).toFixed(1) + 'K' : v.toFixed(2))
        },
        splitLine: { show: false }
      }
    ]
  }

  const gridTop = isByModel ? 16 : 16
  const gridRight = isByModel ? 24 : 60
  const gridLeft = isByModel ? 60 : 60

  chart.setOption({
    grid: { top: gridTop, right: gridRight, bottom: 28, left: gridLeft },
    legend: isByModel ? {
      show: false
    } : { show: false },
    xAxis: {
      type: 'category',
      data: props.dates,
      axisLabel: { fontSize: 10, color: textMuted },
      axisTick: { show: false },
      axisLine: { lineStyle: { color: borderColor } }
    },
    yAxis: yAxisOptions,
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

// 局部更新：仅 hover 压暗态变化时，只刷新受影响 series 的透明度，不重建坐标轴/网格
function updateDimming(): void {
  if (!chart || props.mode !== 'byModel') return
  chart.setOption({ series: props.modelSeries.map(s => makeModelSeries(s)) }, false)
}

let resizeObserver: ResizeObserver | null = null

onMounted(() => {
  window.addEventListener('resize', handleResize)
  resizeObserver = new ResizeObserver(() => {
    if (chart) {
      chart.resize()
    } else if (chartRef.value && chartRef.value.clientWidth > 0 && chartRef.value.clientHeight > 0) {
      renderChart()
    }
  })
  if (chartRef.value) resizeObserver.observe(chartRef.value)
})

// 需要全量重建坐标轴/系列的依赖：精确列出，避免 dimmedModels 等局部变化触发整图重绘
watch(
  [
    () => props.dates,
    () => props.totalCostData,
    () => props.totalTokenData,
    () => props.inputData,
    () => props.outputData,
    () => props.cacheReadData,
    () => props.cacheCreationData,
    () => props.visibleSeries,
    () => props.mode,
    () => props.modelSeries
  ],
  renderChart,
  { deep: true, immediate: true }
)
watch(() => props.dimmedModels, updateDimming, { deep: true })
watch(() => themeStore.isDark, renderChart)

onUnmounted(() => {
  window.removeEventListener('resize', handleResize)
  resizeObserver?.disconnect()
  resizeObserver = null
  if (chart) { chart.dispose(); chart = null }
})

function handleResize(): void { chart?.resize() }
</script>

<style scoped>
.trend-chart {
  width: 100%;
  height: 100%;
  /* 抵消 body zoom: 1.1，修复 ECharts tooltip 坐标偏移 */
  zoom: calc(1 / 1.1);
}
</style>
