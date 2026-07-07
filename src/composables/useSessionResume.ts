import { platformAdapter } from '@/platform'

export type ResumableSourceType = 'claude' | 'opencode' | 'codex' | string | undefined

const RESUME_LABELS = { claude: 'Claude', opencode: 'OpenCode', codex: 'Codex' } as const

// 按 sourceType 分发到对应的 resume*Session 调用，统一日志/错误上报，避免三处会话视图各自复制一份
export function useSessionResume(logTag: string, onError?: (label: string, err: any) => void) {
  async function resumeSession(sourceType: ResumableSourceType, sessionId: string, projectDir?: string): Promise<void> {
    const type = sourceType === 'opencode' || sourceType === 'codex' ? sourceType : 'claude'
    const label = RESUME_LABELS[type]
    try {
      if (type === 'opencode') await platformAdapter.resumeOpenCodeSession(sessionId, projectDir)
      else if (type === 'codex') await platformAdapter.resumeCodexSession(sessionId, projectDir)
      else await platformAdapter.resumeClaudeSession(sessionId, projectDir)
    } catch (err: any) {
      console.error(`[${logTag}] 恢复 ${label} 失败:`, err?.message || err)
      onError?.(label, err)
    }
  }

  return { resumeSession }
}
