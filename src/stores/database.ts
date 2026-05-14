import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { SourceInfo } from '@/platform/types'

export const useDatabaseStore = defineStore('database', () => {
  const sources = ref<SourceInfo[]>([])
  const isLoading = ref(false)
  const isLoaded = ref(false)
  const error = ref('')
  const refreshVersion = ref(0)

  const hasDatabase = computed(() => sources.value.length > 0)
  const dbPath = computed(() => sources.value.map(s => s.path).join(', '))
  const recordCount = computed(() => sources.value.reduce((s, d) => s + d.recordCount, 0))

  function setLoading(loading: boolean): void {
    isLoading.value = loading
  }

  function setSources(list: SourceInfo[]): void {
    sources.value = list
    isLoaded.value = list.length > 0
    isLoading.value = false
    error.value = ''
  }

  function setError(msg: string): void {
    error.value = msg
    isLoading.value = false
  }

  function reset(): void {
    sources.value = []
    isLoaded.value = false
    isLoading.value = false
    error.value = ''
  }

  return {
    sources,
    isLoading,
    isLoaded,
    error,
    hasDatabase,
    dbPath,
    recordCount,
    refreshVersion,
    setLoading,
    setSources,
    setError,
    reset
  }
})
