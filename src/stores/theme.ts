import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useThemeStore = defineStore('theme', () => {
  const isDark = ref(false)

  function init(): void {
    const saved = localStorage.getItem('theme')
    isDark.value = saved === 'dark'
    applyClass()
  }

  function toggle(): void {
    isDark.value = !isDark.value
    localStorage.setItem('theme', isDark.value ? 'dark' : 'light')
    applyClass()
  }

  function applyClass(): void {
    document.documentElement.classList.toggle('dark', isDark.value)
  }

  return { isDark, init, toggle }
})
