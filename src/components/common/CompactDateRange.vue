<template>
  <div class="compact-daterange" ref="triggerRef">
    <div class="compact-daterange__trigger" :class="{ disabled }" @click="toggle">
      <span v-if="!value" class="compact-daterange__placeholder">选择日期范围</span>
      <span v-else class="compact-daterange__value">{{ formatDate(value[0]) }} ~ {{ formatDate(value[1]) }}</span>
      <span v-if="value" class="compact-daterange__clear" @click.stop="emit('update:value', null)">✕</span>
      <span v-else class="compact-daterange__arrow">▾</span>
    </div>
  </div>
  <Teleport to="body">
    <div v-if="open" class="compact-daterange-overlay" @click="open = false" @contextmenu.prevent="open = false" />
    <div v-if="open" class="compact-daterange-panel" :style="panelStyle">
      <div class="cal-header">
        <button class="cal-nav" @click="prevMonth">&lsaquo;</button>
        <span class="cal-title">{{ calYear }}年{{ calMonth }}月</span>
        <button class="cal-nav" @click="nextMonth">&rsaquo;</button>
      </div>
      <div class="cal-weekdays">
        <span v-for="d in weekdays" :key="d" class="cal-wd">{{ d }}</span>
      </div>
      <div class="cal-days">
        <button
          v-for="(day, i) in calDays"
          :key="i"
          class="cal-day"
          :class="dayClass(day)"
          :disabled="!day.current"
          @click="onDayClick(day)"
        >{{ day.num }}</button>
      </div>
      <div v-if="pendingStart" class="cal-hint">请选择结束日期</div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onBeforeUnmount } from 'vue'

const props = defineProps<{
  value: [number, number] | null
}>()

const emit = defineEmits<{ 'update:value': [value: [number, number] | null] }>()

const disabled = defineModel<boolean>('disabled', { default: false })

const open = ref(false)
const pendingStart = ref<number | null>(null)
const triggerRef = ref<HTMLElement>()
const panelStyle = ref<Record<string, string>>({})

const weekdays = ['一', '二', '三', '四', '五', '六', '日']

const now = new Date()
const calYear = ref(now.getFullYear())
const calMonth = ref(now.getMonth() + 1)

interface CalDay { num: number; current: boolean; date: Date }

const calDays = computed<CalDay[]>(() => {
  const y = calYear.value
  const m = calMonth.value
  const firstDay = new Date(y, m - 1, 1)
  const lastDay = new Date(y, m, 0)
  const daysInMonth = lastDay.getDate()
  // 周一=0 ... 周日=6
  let startWd = firstDay.getDay() - 1
  if (startWd < 0) startWd = 6

  const days: CalDay[] = []
  // 上月填充
  const prevLast = new Date(y, m - 1, 0).getDate()
  for (let i = startWd - 1; i >= 0; i--) {
    days.push({ num: prevLast - i, current: false, date: new Date(y, m - 2, prevLast - i) })
  }
  // 当月
  for (let d = 1; d <= daysInMonth; d++) {
    days.push({ num: d, current: true, date: new Date(y, m - 1, d) })
  }
  // 下月填充到 6 行
  const total = Math.ceil(days.length / 7) * 7
  for (let d = 1; days.length < total; d++) {
    days.push({ num: d, current: false, date: new Date(y, m, d) })
  }
  return days
})

function dayClass(day: CalDay) {
  if (!day.current) return 'other'
  const ts = day.date.getTime()
  const sel = props.value
  const ps = pendingStart.value
  const classes: string[] = []
  if (sel) {
    const s = startOfDay(sel[0])
    const e = startOfDay(sel[1])
    if (ts === s) classes.push('range-start')
    if (ts === e) classes.push('range-end')
    if (ts > s && ts < e) classes.push('range-mid')
  }
  if (ps && ts === startOfDay(ps)) classes.push('pending')
  if (ts === todayTs()) classes.push('today')
  return classes.join(' ')
}

function onDayClick(day: CalDay) {
  if (!day.current) return
  const ts = startOfDay(day.date.getTime())
  if (pendingStart.value === null) {
    pendingStart.value = ts
  } else {
    let s = pendingStart.value
    let e = ts
    if (s > e) { const tmp = s; s = e; e = tmp }
    emit('update:value', [s, e + 86400000 - 1])
    pendingStart.value = null
    open.value = false
  }
}

function prevMonth() {
  if (calMonth.value === 1) { calMonth.value = 12; calYear.value-- }
  else calMonth.value--
}
function nextMonth() {
  if (calMonth.value === 12) { calMonth.value = 1; calYear.value++ }
  else calMonth.value++
}

function toggle() {
  if (disabled.value) return
  if (open.value) { open.value = false; return }
  pendingStart.value = null
  // 跳到选中月份或当前月
  if (props.value) {
    const d = new Date(props.value[0])
    calYear.value = d.getFullYear()
    calMonth.value = d.getMonth() + 1
  } else {
    const n = new Date()
    calYear.value = n.getFullYear()
    calMonth.value = n.getMonth() + 1
  }
  open.value = true
  nextTick(positionPanel)
}

function positionPanel() {
  const el = triggerRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const zoom = parseFloat(getComputedStyle(document.body).zoom) || 1
  const max_x = window.innerWidth / zoom - 8
  const max_y = window.innerHeight / zoom - 8
  let x = rect.left / zoom
  let y = (rect.bottom + 2) / zoom
  const pw = 230
  const ph = 280
  if (x + pw > max_x) x = Math.max(8, max_x - pw)
  if (y + ph > max_y) y = Math.max(8, rect.top / zoom - ph - 2)
  panelStyle.value = { left: x + 'px', top: y + 'px' }
}

function startOfDay(ts: number): number {
  const d = new Date(ts)
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}
function todayTs(): number { return startOfDay(Date.now()) }
function formatDate(ts: number): string {
  if (ts === 0) return '永久'
  const d = new Date(ts)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

function onGlobalKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) open.value = false
}
onMounted(() => window.addEventListener('keydown', onGlobalKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onGlobalKeydown))
</script>

<style>
.compact-daterange-overlay {
  position: fixed; inset: 0; z-index: 10001;
}
.compact-daterange-panel {
  position: fixed; z-index: 10002;
  width: 224px;
  background: var(--bg-card);
  border: 1px solid var(--border-main);
  border-radius: 6px;
  box-shadow: 0 2px 10px rgba(0,0,0,0.12);
  padding: 8px;
  font-size: 11px;
  user-select: none;
}
.cal-header {
  display: flex; align-items: center; justify-content: space-between;
  margin-bottom: 6px;
}
.cal-title { font-size: 12px; font-weight: 600; color: var(--text-primary); }
.cal-nav {
  width: 22px; height: 22px;
  border: none; background: none; border-radius: 3px;
  font-size: 14px; color: var(--text-secondary); cursor: pointer;
  display: flex; align-items: center; justify-content: center;
}
.cal-nav:hover { background: var(--bg-hover); color: var(--color-blue); }
.cal-weekdays {
  display: grid; grid-template-columns: repeat(7, 1fr);
  text-align: center; margin-bottom: 2px;
}
.cal-wd { font-size: 10px; color: var(--text-muted); padding: 2px 0; }
.cal-days {
  display: grid; grid-template-columns: repeat(7, 1fr);
  gap: 1px;
}
.cal-day {
  height: 26px; border: none; background: none; border-radius: 3px;
  font-size: 11px; color: var(--text-primary); cursor: pointer;
  display: flex; align-items: center; justify-content: center;
  transition: background 0.12s;
}
.cal-day:hover { background: var(--bg-hover); }
.cal-day.other { color: var(--text-faint); pointer-events: none; }
.cal-day.today { color: var(--color-blue); font-weight: 600; }
.cal-day.range-start,
.cal-day.range-end {
  background: var(--color-blue); color: #fff; border-radius: 3px;
}
.cal-day.range-mid { background: var(--color-blue-bg); border-radius: 0; }
.cal-day.pending {
  background: rgba(74,144,217,0.3); border-radius: 3px;
}
.cal-hint {
  text-align: center; font-size: 10px; color: var(--color-blue);
  padding: 4px 0 0;
}
</style>

<style scoped>
.compact-daterange { display: inline-block; }
.compact-daterange__trigger {
  display: inline-flex; align-items: center; gap: 2px;
  font-size: 11px; color: var(--text-secondary); cursor: pointer;
  padding: 1px 4px; border: 1px solid var(--border-main); border-radius: 3px;
  background: var(--bg-card); min-width: 80px; max-width: 300px;
  height: 20px; line-height: 20px; transition: border-color 0.15s;
}
.compact-daterange__trigger:hover { border-color: var(--color-blue); }
.compact-daterange__trigger.disabled { opacity: 0.5; cursor: not-allowed; background: var(--bg-hover); }
.compact-daterange__trigger.disabled:hover { border-color: var(--border-main); }
.compact-daterange__placeholder { flex: 1; color: var(--text-muted); }
.compact-daterange__value { flex: 1; white-space: nowrap; }
.compact-daterange__clear { font-size: 10px; color: var(--text-muted); }
.compact-daterange__clear:hover { color: var(--text-primary); }
.compact-daterange__arrow { font-size: 10px; color: var(--text-muted); }
</style>
