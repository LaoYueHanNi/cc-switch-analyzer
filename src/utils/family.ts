import type { PricingData, PricingFamily } from '@/types/pricing'

export const OTHER_FAMILY: PricingFamily = { id: 'other', label: '其他' }

/** 已合并/废弃的 family id，不再出现在筛选与分组列表 */
export const DEPRECATED_FAMILY_IDS = new Set(['cursor', 'grok', 'composer'])

export const LEGACY_FAMILY_MAP: Record<string, string> = {
  grok: 'spacex-ai',
  composer: 'spacex-ai',
  cursor: 'gpt'
}

/** 家族列表（含兜底「其他」，过滤已废弃 id） */
export function getEffectiveFamilies(raw: PricingFamily[]): PricingFamily[] {
  const base = raw.length > 0 ? raw : [OTHER_FAMILY]
  const list = base.filter(f => !DEPRECATED_FAMILY_IDS.has(f.id))
  if (list.some(f => f.id === 'other')) return list
  return [...list, OTHER_FAMILY]
}

/** 将定价表上的 family 字段解析为有效 family id */
export function resolveFamilyId(
  familyRaw: string | undefined | null,
  effectiveFamilies: PricingFamily[]
): string {
  const fam = familyRaw?.trim()
  if (!fam) return 'other'
  const mapped = LEGACY_FAMILY_MAP[fam] ?? fam
  return effectiveFamilies.some(f => f.id === mapped) ? mapped : 'other'
}

export function familyLabel(id: string, families: PricingFamily[]): string {
  return families.find(f => f.id === id)?.label || id
}

/** modelId / alias → familyId（含未定价模型 → other） */
export function buildModelFamilyMap(
  pricingList: PricingData[],
  effectiveFamilies: PricingFamily[]
): Map<string, string> {
  const map = new Map<string, string>()
  for (const p of pricingList) {
    const fid = resolveFamilyId(p.family, effectiveFamilies)
    map.set(p.modelId, fid)
    for (const alias of p.aliases || []) map.set(alias, fid)
    for (const alias of p.userAliases || []) map.set(alias, fid)
  }
  return map
}

export function lookupModelFamily(
  modelId: string,
  modelFamilyMap: Map<string, string>
): string {
  return modelFamilyMap.get(modelId) || 'other'
}
