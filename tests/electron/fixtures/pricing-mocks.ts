import type { ContextTier, PricingOverride, TimePricingRule, CloudPricingTimeRule, ModelPricingRow } from '../../electron/main/services/app-db'
import type { MergedPricing, TokenDimensions } from '../../electron/main/services/pricing-engine'

// ========== MergedPricing ==========

export function makeMergedPricing(overrides: Partial<MergedPricing> = {}): MergedPricing {
  return {
    modelId: 'claude-sonnet-4-20250514',
    inputCostPerMillion: 21,
    outputCostPerMillion: 105,
    cacheReadCostPerMillion: 2.1,
    cacheCreationCostPerMillion: 26.25,
    isOverride: false,
    ...overrides
  }
}

// ========== ContextTier ==========

export function makeContextTier(overrides: Partial<ContextTier> = {}): ContextTier {
  return {
    threshold: 10000,
    inputCostPerMillion: 21,
    outputCostPerMillion: 105,
    cacheReadCostPerMillion: 2.1,
    cacheCreationCostPerMillion: 26.25,
    ...overrides
  }
}

// ========== ModelPricingRow (cloud base) ==========

export function makeModelPricingRow(overrides: Partial<ModelPricingRow> = {}): ModelPricingRow {
  return {
    modelId: 'claude-sonnet-4-20250514',
    inputCostPerMillion: 21,
    outputCostPerMillion: 105,
    cacheReadCostPerMillion: 2.1,
    cacheCreationCostPerMillion: 26.25,
    aliases: [],
    ...overrides
  }
}

// ========== PricingOverride ==========

export function makePricingOverride(overrides: Partial<PricingOverride> = {}): PricingOverride {
  return {
    modelId: 'claude-sonnet-4-20250514',
    inputCostPerMillion: 30,
    outputCostPerMillion: 150,
    cacheReadCostPerMillion: 3,
    cacheCreationCostPerMillion: 37.5,
    updatedAt: 1700000000,
    contextTiers: [],
    ...overrides
  }
}

// ========== TimePricingRule ==========

export function makeTimeRule(overrides: Partial<TimePricingRule> = {}): TimePricingRule {
  return {
    id: 1,
    modelId: 'claude-sonnet-4-20250514',
    startTime: 1700000000,
    endTime: 1700086400,
    inputCostPerMillion: 10.5,
    outputCostPerMillion: 52.5,
    cacheReadCostPerMillion: 1.05,
    cacheCreationCostPerMillion: 13.125,
    label: '夜间折扣',
    contextTiers: [],
    ...overrides
  }
}

// ========== CloudPricingTimeRule ==========

export function makeCloudTimeRule(overrides: Partial<CloudPricingTimeRule> = {}): CloudPricingTimeRule {
  return {
    label: '云端折扣',
    startTime: 1700000000,
    endTime: 1700086400,
    inputCostPerMillion: 15,
    outputCostPerMillion: 75,
    cacheReadCostPerMillion: 1.5,
    cacheCreationCostPerMillion: 18.75,
    contextTiers: [],
    ...overrides
  }
}

// ========== TokenDimensions ==========

export function makeTokens(overrides: Partial<TokenDimensions> = {}): TokenDimensions {
  return {
    input: 1_000_000,
    output: 500_000,
    cacheRead: 200_000,
    cacheCreation: 100_000,
    ...overrides
  }
}

// ========== AppDbService Mock ==========

export interface AppDbMockConfig {
  cloudBase?: ModelPricingRow[]
  cloudTiers?: Map<string, ContextTier[]>
  cloudTimeRules?: Map<string, CloudPricingTimeRule[]>
  overrides?: PricingOverride[]
  timeOverrides?: TimePricingRule[]
  settings?: Record<string, string>
}

export function makeAppDbMock(config: AppDbMockConfig = {}) {
  const settings = new Map(Object.entries(config.settings || {}))

  return {
    loadCloudPricing: () => {
      const base = config.cloudBase || []
      const cloudAliases = new Map<string, string[]>()
      for (const row of base) {
        cloudAliases.set(row.modelId, row.aliases || [])
      }
      return {
        base,
        tiers: config.cloudTiers || new Map(),
        cloudTimeRules: config.cloudTimeRules || new Map(),
        cloudAliases
      }
    },
    getAllOverrides: () => config.overrides || [],
    getAllTimeOverrides: () => config.timeOverrides || [],
    getUserAliases: () => new Map<string, string[]>(),
    getSetting: (key: string) => settings.get(key) ?? null
  }
}
