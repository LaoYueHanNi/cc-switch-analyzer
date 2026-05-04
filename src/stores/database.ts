import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

// 数据库状态管理
export const useDatabaseStore = defineStore('database', () => {
  const dbPath = ref('')
  const recordCount = ref(0)
  const isLoading = ref(false)
  const isLoaded = ref(false)
  const error = ref('')
  const refreshVersion = ref(0)

  const hasDatabase = computed(() => isLoaded.value && dbPath.value !== '')

  function setLoading(loading: boolean): void {
    isLoading.value = loading
  }

  function setLoaded(path: string, count: number): void {
    dbPath.value = path
    recordCount.value = count
    isLoaded.value = true
    isLoading.value = false
    error.value = ''
  }

  function setError(msg: string): void {
    error.value = msg
    isLoading.value = false
  }

  function reset(): void {
    dbPath.value = ''
    recordCount.value = 0
    isLoaded.value = false
    isLoading.value = false
    error.value = ''
  }

  return {
    dbPath,
    recordCount,
    isLoading,
    isLoaded,
    error,
    hasDatabase,
    refreshVersion,
    setLoading,
    setLoaded,
    setError,
    reset
  }
})
