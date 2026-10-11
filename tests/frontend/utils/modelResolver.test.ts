import { describe, expect, it, vi } from 'vitest'
import fixture from '../../fixtures/model-aliases.json'
import { compileAliasPattern, createModelResolver, foldModelName } from '@/utils/modelResolver'
import { buildModelFamilyMap, lookupModelFamily } from '@/utils/family'
import type { PricingData } from '@/types/pricing'

describe('model resolution shared with Rust', () => {
  it('matches canonical IDs, historical/user names and full regex names in ASCII case variants', () => {
    const resolver = createModelResolver(fixture.models)
    for (const [name, expected] of fixture.cases) {
      for (const variant of [name!, name!.toUpperCase()]) {
        expect(resolver.get(variant)?.modelId ?? null, variant).toBe(expected)
      }
    }
    expect(foldModelName('ABCÉ')).toBe('abcÉ')
  })

  it('keeps global stage priority even in reverse model order', () => {
    const models = [
      { modelId: 'Example' },
      { modelId: 'alias-owner', aliases: ['EXAMPLE'] },
      { modelId: 'regex-owner', aliasPatterns: ['^example$'] }
    ]
    expect(createModelResolver([...models].reverse()).get('example')?.modelId).toBe('Example')
    expect(createModelResolver(models.slice(1).reverse()).get('example')?.modelId).toBe('alias-owner')
    expect(createModelResolver(models.slice(2)).get('example')?.modelId).toBe('regex-owner')
  })

  it('reports ambiguity in every stage and deduplicates one model matching multiple patterns', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    for (const models of [
      [{ modelId: 'Name' }, { modelId: 'NAME' }],
      [{ modelId: 'a', aliases: ['name'] }, { modelId: 'b', userAliases: ['NAME'] }],
      [{ modelId: 'a', aliasPatterns: ['^name$'] }, { modelId: 'b', aliasPatterns: ['^NAME$'] }]
    ]) {
      const resolver = createModelResolver(models)
      expect(resolver.resolve('NaMe').error).toContain('歧义')
      expect(resolver.get('NaMe')).toBeUndefined()
    }
    expect(createModelResolver([{ modelId: 'a', aliasPatterns: ['^name$', '^NAME$'] }]).get('Name')?.modelId).toBe('a')
    log.mockRestore()
  })

  it('rejects invalid/unportable patterns and duplicate patterns', () => {
    for (const pattern of ['', '^$', 'name', '^a.*$', '^a+$', '^a{2}$', '^(?=a)a$', '^a\\1$', '(?i)^a$', '^a.$', '^a($']) {
      expect(() => compileAliasPattern(pattern), pattern).toThrow()
    }
    expect(() => createModelResolver([{ modelId: 'a', aliasPatterns: ['^a$', '^a$'] }])).toThrow('重复')
  })

  it('assigns regex and user aliases to the canonical family; unknown models stay other', () => {
    const models = fixture.models.map(m => ({ ...m, family: 'gpt' })) as PricingData[]
    const map = buildModelFamilyMap(models, [{ id: 'gpt', label: 'GPT' }])
    expect(lookupModelFamily('GROK-4.7-HIGH-FAST', map)).toBe('gpt')
    expect(lookupModelFamily('MY-MODEL', map)).toBe('gpt')
    expect(lookupModelFamily('unknown', map)).toBe('other')
  })
})
