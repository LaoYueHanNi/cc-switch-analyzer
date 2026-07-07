import { reactive } from 'vue'
import { platformAdapter } from '@/platform'

export interface ProviderMenuItem {
  id: string
  name: string
}

const DROPDOWN_WIDTH = 180
const MARGIN = 8

function clampPosition(x: number, y: number): { x: number; y: number } {
  const vw = window.innerWidth
  const vh = window.innerHeight
  return { x: Math.min(x, vw - DROPDOWN_WIDTH - MARGIN), y: Math.min(y, vh - MARGIN) }
}

function menuPositionFromElement(el: HTMLElement): { x: number; y: number } {
  const rect = el.getBoundingClientRect()
  const zoom = parseFloat(getComputedStyle(document.body).zoom) || 1
  const maxX = window.innerWidth / zoom - MARGIN
  let x = rect.left / zoom
  const y = (rect.bottom + 4) / zoom
  if (x + 130 > maxX) x = Math.max(MARGIN, rect.right / zoom - 130)
  return { x, y }
}

// 供应商右键菜单：定位、显隐状态与"获取可用供应商列表"逻辑
// 用于 SessionAnalysis / TaskDetail / Task 三处会话视图，配合 ProviderContextMenu.vue 使用
export function useProviderContextMenu(logTag: string) {
  const menu = reactive({
    show: false,
    x: 0,
    y: 0,
    items: [] as ProviderMenuItem[]
  })

  let selectHandler: ((providerId: string) => void) | null = null

  async function loadProviderItems(dbPath: string | undefined): Promise<ProviderMenuItem[]> {
    if (!dbPath) return []
    try {
      const providers = await platformAdapter.getCcswitchProviders(dbPath)
      return providers.filter(p => p.hasEnv).map(p => ({ id: p.id, name: p.name }))
    } catch (err: any) {
      console.error(`[${logTag}] 获取供应商列表失败:`, err?.message || err)
      return []
    }
  }

  // event.target 优先取触发按钮位置，取不到时退回鼠标位置
  function openMenu(event: MouseEvent, items: ProviderMenuItem[], onSelect: (providerId: string) => void): boolean {
    if (items.length === 0) return false
    menu.items = items
    selectHandler = onSelect
    const el = (event.target as HTMLElement).closest('button, .action-terminal') as HTMLElement | null
    const pos = el ? menuPositionFromElement(el) : clampPosition(event.clientX, event.clientY)
    menu.x = pos.x
    menu.y = pos.y
    menu.show = true
    return true
  }

  // 菜单渲染后按实际高度做一次纵向越界校正
  function adjustMenuPosition(menuEl: HTMLElement | null): void {
    if (!menuEl) return
    const zoom = parseFloat(getComputedStyle(document.body).zoom) || 1
    const maxY = window.innerHeight / zoom - MARGIN
    const bottom = menu.y + menuEl.offsetHeight
    if (bottom > maxY) menu.y = Math.max(MARGIN, maxY - menuEl.offsetHeight)
  }

  function selectItem(providerId: string): void {
    menu.show = false
    selectHandler?.(providerId)
  }

  function closeMenu(): void {
    menu.show = false
  }

  return { menu, loadProviderItems, openMenu, adjustMenuPosition, selectItem, closeMenu }
}
