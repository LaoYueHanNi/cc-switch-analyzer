/**
 * 从 CSS 变量读取颜色值
 */
export function getColor(name: string): string {
  const cs = getComputedStyle(document.documentElement)
  return cs.getPropertyValue(name).trim()
}

/**
 * hex 颜色转 rgba
 */
export function hexToRgba(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return `rgba(${r},${g},${b},${alpha})`
}
