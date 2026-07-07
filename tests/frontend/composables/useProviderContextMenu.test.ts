import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useProviderContextMenu } from '@/composables/useProviderContextMenu'
import { platformAdapter } from '@/platform'

vi.mock('@/platform', () => ({
  platformAdapter: {
    getCcswitchProviders: vi.fn()
  }
}))

// ---------------------------------------------------------------------------
// loadProviderItems — 获取 + 过滤（hasEnv）+ 出错兜底，不依赖 DOM
// ---------------------------------------------------------------------------
describe('useProviderContextMenu.loadProviderItems', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('dbPath 为空时直接返回空数组，不发起请求', async () => {
    const { loadProviderItems } = useProviderContextMenu('Test')
    const items = await loadProviderItems(undefined)
    expect(items).toEqual([])
    expect(platformAdapter.getCcswitchProviders).not.toHaveBeenCalled()
  })

  it('只保留 hasEnv 为 true 的供应商，并映射为 { id, name }', async () => {
    vi.mocked(platformAdapter.getCcswitchProviders).mockResolvedValue([
      { id: 'p1', name: 'Provider 1', hasEnv: true },
      { id: 'p2', name: 'Provider 2', hasEnv: false },
      { id: 'p3', name: 'Provider 3', hasEnv: true }
    ])
    const { loadProviderItems } = useProviderContextMenu('Test')
    const items = await loadProviderItems('/path/to/db')
    expect(items).toEqual([
      { id: 'p1', name: 'Provider 1' },
      { id: 'p3', name: 'Provider 3' }
    ])
  })

  it('请求失败时返回空数组，不向上抛出', async () => {
    vi.mocked(platformAdapter.getCcswitchProviders).mockRejectedValue(new Error('boom'))
    const { loadProviderItems } = useProviderContextMenu('Test')
    const items = await loadProviderItems('/path/to/db')
    expect(items).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// selectItem / closeMenu — 纯状态流转，不依赖 DOM
// ---------------------------------------------------------------------------
describe('useProviderContextMenu 菜单状态', () => {
  it('closeMenu 应将 show 置为 false', () => {
    const { menu, closeMenu } = useProviderContextMenu('Test')
    menu.show = true
    closeMenu()
    expect(menu.show).toBe(false)
  })

  it('未 openMenu 时 selectItem 不应抛出异常', () => {
    const { selectItem, menu } = useProviderContextMenu('Test')
    expect(() => selectItem('provider-x')).not.toThrow()
    expect(menu.show).toBe(false)
  })
})
