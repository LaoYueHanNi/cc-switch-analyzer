import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useSessionResume } from '@/composables/useSessionResume'
import { platformAdapter } from '@/platform'

vi.mock('@/platform', () => ({
  platformAdapter: {
    resumeClaudeSession: vi.fn(),
    resumeOpenCodeSession: vi.fn(),
    resumeCodexSession: vi.fn()
  }
}))

describe('useSessionResume.resumeSession', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('sourceType 为 opencode 时调用 resumeOpenCodeSession', async () => {
    const { resumeSession } = useSessionResume('Test')
    await resumeSession('opencode', 'sess-1', '/proj')
    expect(platformAdapter.resumeOpenCodeSession).toHaveBeenCalledWith('sess-1', '/proj')
    expect(platformAdapter.resumeClaudeSession).not.toHaveBeenCalled()
  })

  it('sourceType 为 codex 时调用 resumeCodexSession', async () => {
    const { resumeSession } = useSessionResume('Test')
    await resumeSession('codex', 'sess-2', undefined)
    expect(platformAdapter.resumeCodexSession).toHaveBeenCalledWith('sess-2', undefined)
  })

  it('sourceType 为空/其他值时默认走 resumeClaudeSession', async () => {
    const { resumeSession } = useSessionResume('Test')
    await resumeSession(undefined, 'sess-3', '/proj')
    expect(platformAdapter.resumeClaudeSession).toHaveBeenCalledWith('sess-3', '/proj')
  })

  it('调用失败时吞掉异常并回调 onError，携带来源标签', async () => {
    vi.mocked(platformAdapter.resumeClaudeSession).mockRejectedValue(new Error('boom'))
    const onError = vi.fn()
    const { resumeSession } = useSessionResume('Test', onError)
    await expect(resumeSession('claude', 'sess-4')).resolves.toBeUndefined()
    expect(onError).toHaveBeenCalledWith('Claude', expect.any(Error))
  })
})
