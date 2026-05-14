import { ref } from 'vue'
import { platformAdapter } from '@/platform'

interface TitleInfo {
  title: string
  project: string
  source?: string
}

export function useSessionTitles() {
  const sessionTitles = ref<Map<string, TitleInfo>>(new Map())

  function getTitle(sessionId: string): string {
    return sessionTitles.value.get(sessionId)?.title || ''
  }

  function getProject(sessionId: string): string {
    return sessionTitles.value.get(sessionId)?.project || ''
  }

  function getSource(sessionId: string): string {
    return sessionTitles.value.get(sessionId)?.source || ''
  }

  async function fetchTitles(sessionIds: string[]): Promise<void> {
    if (sessionIds.length === 0) return
    const uncached = sessionIds.filter(id => !sessionTitles.value.has(id))
    if (uncached.length === 0) return
    try {
      const titles = await platformAdapter.getSessionTitles(uncached)
      const newMap = new Map(sessionTitles.value)
      for (const [id, info] of Object.entries(titles)) {
        newMap.set(id, typeof info === 'string' ? { title: info, project: '' } : info)
      }
      sessionTitles.value = newMap
    } catch (err) {
      console.error('获取会话标题失败:', err)
    }
  }

  return { sessionTitles, getTitle, getProject, getSource, fetchTitles }
}
