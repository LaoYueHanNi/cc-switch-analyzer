export interface ModelIdentity {
  modelId: string
  aliases?: string[]
  userAliases?: string[]
  aliasPatterns?: string[]
}

export const foldModelName = (name: string): string =>
  name.replace(/[A-Z]/g, letter => letter.toLowerCase())

export function compileAliasPattern(pattern: string): RegExp {
  const invalid = () => new Error(`非法模型别名正则: ${pattern}`)
  if (!pattern.startsWith('^') || !pattern.endsWith('$') || pattern.length < 3) throw invalid()
  const body = pattern.slice(1, -1)
  for (let i = 0; i < body.length; i++) {
    const char = body[i]
    if (char === '\\') {
      const literal = body[++i]
      if (!literal || !/[!-/:-@\[-`{-~]/.test(literal)) throw invalid()
    } else if (char === '(' && body[i + 1] === '?') {
      throw invalid()
    } else if (!/[A-Za-z0-9_:/ ()|?\-]/.test(char)) {
      throw invalid()
    }
  }
  return new RegExp(pattern, 'i')
}

/** Compile once per pricing snapshot; all consumers use the same global stages. */
export function createModelResolver<T extends ModelIdentity>(models: T[]) {
  const ids = new Map<string, Set<T>>()
  const aliases = new Map<string, Set<T>>()
  const add = (map: Map<string, Set<T>>, name: string, model: T) => {
    const key = foldModelName(name)
    if (!map.has(key)) map.set(key, new Set())
    map.get(key)!.add(model)
  }
  const patterns = models.map(model => {
    add(ids, model.modelId, model)
    for (const alias of [...(model.aliases || []), ...(model.userAliases || [])]) add(aliases, alias, model)
    const values = model.aliasPatterns || []
    if (new Set(values).size !== values.length) throw new Error(`模型 ${model.modelId} 的别名正则重复`)
    return { model, regexes: values.map(compileAliasPattern) }
  })
  const cache = new Map<string, { model?: T; error?: string }>()
  function resolve(name: string): { model?: T; error?: string } {
    if (cache.has(name)) return cache.get(name)!
    const key = foldModelName(name)
    const hits = ids.get(key) || aliases.get(key) || new Set(patterns
      .filter(({ regexes }) => regexes.some(regex => {
        const match = regex.exec(name)
        return match?.index === 0 && match[0].length === name.length
      })).map(({ model }) => model))
    const result = hits.size > 1
      ? { error: `模型别名存在歧义: ${name} → ${[...hits].map(m => m.modelId).sort().join(', ')}` }
      : { model: hits.values().next().value as T | undefined }
    if (result.error) console.error(result.error)
    if (cache.size >= 4096) cache.clear()
    cache.set(name, result)
    return result
  }
  return { resolve, get: (name: string): T | undefined => resolve(name).model }
}
