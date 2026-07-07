import { defineStore } from 'pinia'
import { ref } from 'vue'
import { platformAdapter } from '@/platform'
import type { TaskDetail, TaskSessionInput, TaskStatus, TaskWithStats } from '@/types/task'

export const useTaskStore = defineStore('task', () => {
  const tasks = ref<TaskWithStats[]>([])
  const currentDetail = ref<TaskDetail | null>(null)
  const isLoading = ref(false)
  const error = ref('')

  function setLoading(loading: boolean): void {
    isLoading.value = loading
  }

  function setError(msg: string): void {
    error.value = msg
    isLoading.value = false
  }

  function clearError(): void {
    error.value = ''
  }

  function setTasks(list: TaskWithStats[]): void {
    tasks.value = list
    isLoading.value = false
  }

  function setDetail(detail: TaskDetail | null): void {
    currentDetail.value = detail
    isLoading.value = false
  }

  async function fetchAll(): Promise<void> {
    setLoading(true)
    clearError()
    try {
      const list = await platformAdapter.listTasks()
      setTasks(list)
    } catch (e) {
      setError(String(e))
    }
  }

  async function fetchDetail(taskId: number): Promise<void> {
    setLoading(true)
    clearError()
    try {
      const detail = await platformAdapter.getTaskDetail(taskId)
      setDetail(detail)
    } catch (e) {
      setError(String(e))
      setDetail(null)
    }
  }

  async function create(input: { title: string; description: string; status: TaskStatus }): Promise<number> {
    clearError()
    try {
      const id = await platformAdapter.createTask(input.title, input.description, input.status)
      await fetchAll()
      return id
    } catch (e) {
      setError(String(e))
      throw e
    }
  }

  async function update(taskId: number, input: { title: string; description: string; status: TaskStatus }): Promise<void> {
    clearError()
    try {
      await platformAdapter.updateTask(taskId, input.title, input.description, input.status)
      // 乐观更新:先在本地 tasks 里替换该任务字段,确保 UI 立刻反映
      const idx = tasks.value.findIndex(t => t.id === taskId)
      if (idx >= 0) {
        tasks.value[idx] = {
          ...tasks.value[idx],
          title: input.title,
          description: input.description,
          status: input.status,
          updatedAt: Math.floor(Date.now() / 1000)
        }
      }
      // 后台异步重新拉取以确保和服务端一致(不阻塞 UI)
      fetchAll().catch(() => {})
      if (currentDetail.value?.id === taskId) {
        await fetchDetail(taskId)
      }
    } catch (e) {
      setError(String(e))
      throw e
    }
  }

  async function remove(taskId: number): Promise<void> {
    clearError()
    try {
      await platformAdapter.deleteTask(taskId)
      if (currentDetail.value?.id === taskId) {
        currentDetail.value = null
      }
      await fetchAll()
    } catch (e) {
      setError(String(e))
      throw e
    }
  }

  async function addSessions(taskId: number, sessions: TaskSessionInput[]): Promise<void> {
    clearError()
    try {
      await platformAdapter.addSessionsToTask(taskId, sessions)
      if (currentDetail.value?.id === taskId) {
        await fetchDetail(taskId)
      }
      await fetchAll()
    } catch (e) {
      setError(String(e))
      throw e
    }
  }

  return {
    tasks,
    currentDetail,
    isLoading,
    error,
    fetchAll,
    fetchDetail,
    create,
    update,
    remove,
    addSessions,
    clearError
  }
})
