<template>
  <div class="compact-datepicker" ref="triggerRef">
    <div class="compact-datepicker__trigger" @click="toggle">
      <span v-if="!modelValue" class="compact-datepicker__placeholder">{{ placeholder }}</span>
      <span v-else class="compact-datepicker__value">{{ displayDate }}</span>
      <span class="compact-datepicker__arrow">▾</span>
    </div>
  </div>
  <Teleport to="body">
    <div v-if="open" class="compact-datepicker-overlay" @click="open = false" @contextmenu.prevent="open = false" />
    <div v-if="open" class="compact-datepicker-panel" :style="panelStyle">
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
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onBeforeUnmount } from 'vue'

const props = withDefaults(defineProps<{
  modelValue: number | null
  placeholder?: string
  disabled?: boolean
}>(), {
  placeholder: '选择日期',
  disabled: false,
})

const emit = defineEmits<{ 'update:modelValue': [value: number | null] }>()

const open = ref(false)
const triggerRef = ref<HTMLElement>()
const panelStyle = ref<Record<string, string>>({})

const weekdays = ['一', '二', '三', '四', '五', '六', '日']

const now = new Date()
const calYear = ref(now.getFullYear())
const calMonth = ref(now.getMonth() + 1)

const displayDate = computed(() => {
  if (!props.modelValue) return ''
  const d = new Date(props.modelValue)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
})

interface CalDay { num: number; current: boolean; date: Date }

const calDays = computed<CalDay[]>(() => {
  const y = calYear.value
  const m = calMonth.value
  const firstDay = new Date(y, m - 1, 1)
  const lastDay = new Date(y, m, 0)
  const daysInMonth = lastDay.getDate()
  let startWd = firstDay.getDay() - 1
  if (startWd < 0) startWd = 6
  const days: CalDay[] = []
  const prevLast = new Date(y, m - 1, 0).getDate()
  for (let i = startWd - 1; i >= 0; i--) {
    days.push({ num: prevLast - i, current: false, date: new Date(y, m - 2, prevLast - i) })
  }
  for (let d = 1; d <= daysInMonth; d++) {
    days.push({ num: d, current: true, date: new Date(y, m - 1, d) })
  }
  const total = Math.ceil(days.length / 7) * 7
  for (let d = 1; days.length < total; d++) {
    days.push({ num: d, current: false, date: new Date(y, m, d) })
  }
  return days
})

function dayClass(day: CalDay) {
  if (!day.current) return 'other'
  const ts = startOfDay(day.date.getTime())
  const sel = props.modelValue ? startOfDay(props.modelValue) : -1
  const classes: string[] = []
  if (ts === sel) classes.push('selected')
  if (ts === todayTs()) classes.push('today')
  return classes.join(' ')
}

function onDayClick(day: CalDay) {
  if (!day.current) return
  const d = day.date
  d.setHours(12, 0, 0, 0)
  emit('update:modelValue', d.getTime())
  open.value = false
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
  if (props.disabled) return
  if (open.value) { open.value = false; return }
  if (props.modelValue) {
    const d = new Date(props.modelValue)
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
  if (x + 224 > max_x) x = Math.max(8, max_x - 224)
  if (y + 240 > max_y) y = Math.max(8, rect.top / zoom - 240 - 2)
  panelStyle.value = { left: x + 'px', top: y + 'px' }
}

function startOfDay(ts: number): number {
  const d = new Date(ts)
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}
function todayTs(): number { return startOfDay(Date.now()) }

function onGlobalKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) open.value = false
}
onMounted(() => window.addEventListener('keydown', onGlobalKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onGlobalKeydown))
</script>

<style>
.compact-datepicker-overlay { position: fixed; inset: 0; z-index: 10001; }
.compact-datepicker-panel {
  position: fixed; z-index: 10002; width: 224px;
  background: var(--bg-card); border: 1px solid var(--border-main);
  border-radius: 6px; box-shadow: 0 2px 10px rgba(0,0,0,0.12);
  padding: 8px; font-size: 11px; user-select: none;
}
.cal-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 6px; }
.cal-title { font-size: 12px; font-weight: 600; color: var(--text-primary); }
.cal-nav {
  width: 22px; height: 22px; border: none; background: none; border-radius: 3px;
  font-size: 14px; color: var(--text-secondary); cursor: pointer;
  display: flex; align-items: center; justify-content: center;
}
.cal-nav:hover { background: var(--bg-hover); color: var(--color-blue); }
.cal-weekdays { display: grid; grid-template-columns: repeat(7, 1fr); text-align: center; margin-bottom: 2px; }
.cal-wd { font-size: 10px; color: var(--text-muted); padding: 2px 0; }
.cal-days { display: grid; grid-template-columns: repeat(7, 1fr); gap: 1px; }
.cal-day {
  height: 26px; border: none; background: none; border-radius: 3px;
  font-size: 11px; color: var(--text-primary); cursor: pointer;
  display: flex; align-items: center; justify-content: center; transition: background 0.12s;
}
.cal-day:hover { background: var(--bg-hover); }
.cal-day.other { color: var(--text-faint); pointer-events: none; }
.cal-day.today { color: var(--color-blue); font-weight: 600; }
.cal-day.selected { background: var(--color-blue); color: #fff; border-radius: 3px; }
</style>

<style scoped>
.compact-datepicker { display: inline-block; }
.compact-datepicker__trigger {
  display: inline-flex; align-items: center; gap: 2px;
  font-size: 11px; color: var(--text-secondary); cursor: pointer;
  padding: 1px 4px; border: 1px solid var(--border-main); border-radius: 3px;
  background: var(--bg-card); width: 150px; height: 20px; line-height: 20px;
  transition: border-color 0.15s;
}
.compact-datepicker__trigger:hover { border-color: var(--color-blue); }
.compact-datepicker__placeholder { flex: 1; color: var(--text-muted); }
.compact-datepicker__value { flex: 1; white-space: nowrap; }
.compact-datepicker__arrow { font-size: 10px; color: var(--text-muted); }
</style>
