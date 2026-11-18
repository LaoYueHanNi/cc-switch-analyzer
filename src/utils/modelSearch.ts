import { foldModelName, type ModelIdentity } from '@/utils/modelResolver'

/**
 * 定价页模型筛选匹配：空词全通过；否则按模型名、历史／用户别名与统一解析器
 * （别名、正则）做 ASCII 大小写不敏感的包含匹配。
 * 抽成纯函数供定价页与测试共用，避免组件内不可测的私有逻辑。
 */
export function matchesModelSearch<T extends ModelIdentity>(
  pricing: T,
  search: string,
  resolver: { get(name: string): ModelIdentity | undefined }
): boolean {
  if (!search) return true
  const folded = foldModelName(search)
  return foldModelName(pricing.modelId).includes(folded)
    || (pricing.aliases || []).some(a => foldModelName(a).includes(folded))
    || (pricing.userAliases || []).some(a => foldModelName(a).includes(folded))
    || resolver.get(search)?.modelId === pricing.modelId
}
