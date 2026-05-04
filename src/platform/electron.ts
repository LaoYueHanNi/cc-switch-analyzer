/// <reference path="../../electron/preload/index.d.ts" />
import type { PlatformAdapter, DbResult, RefreshResult, FilterParams, PricingOverrideData, TimePricingRuleData, UpdateTimePricingRuleData } from './types'

export const platformAdapter: PlatformAdapter = {
  // 数据库
  async selectDatabase(): Promise<DbResult | null> {
    return window.api.selectDatabase()
  },
  async autoLoadDatabase(): Promise<DbResult | null> {
    return window.api.autoLoadDatabase()
  },
  async refreshDatabase(): Promise<RefreshResult> {
    return window.api.refreshDatabase()
  },
  // 查询 — Electron 直接传 Date 对象
  async querySummary(params: FilterParams) {
    return window.api.querySummary(params)
  },
  async queryByModel(params: FilterParams) {
    return window.api.queryByModel(params)
  },
  async queryByProvider(params: FilterParams) {
    return window.api.queryByProvider(params)
  },
  async queryPrecompute(params: FilterParams) {
    return window.api.queryPrecompute(params)
  },
  async queryRealtime() {
    return window.api.queryRealtime()
  },
  async queryRealtimeLogs(since?: number) {
    return window.api.queryRealtimeLogs(since)
  },
  async queryCacheWindows(modelId: string) {
    return window.api.queryCacheWindows(modelId)
  },
  async querySessionsWithCost(params: FilterParams) {
    return window.api.querySessionsWithCost(params)
  },
  // 定价
  async getExchangeRate() {
    return window.api.getExchangeRate()
  },
  async setExchangeRate(rate: number) {
    return window.api.setExchangeRate(rate)
  },
  async getAllPricing() {
    return window.api.getAllPricing()
  },
  async setPricingOverride(data: PricingOverrideData) {
    return window.api.setPricingOverride(data)
  },
  async removePricingOverride(modelId: string) {
    return window.api.removePricingOverride(modelId)
  },
  async addTimePricingRule(data: TimePricingRuleData) {
    return window.api.addTimePricingRule(data)
  },
  async updateTimePricingRule(data: UpdateTimePricingRuleData) {
    return window.api.updateTimePricingRule(data)
  },
  async deleteTimePricingRule(id: number) {
    return window.api.deleteTimePricingRule(id)
  },
  async refreshPricing() {
    return window.api.refreshPricing()
  },
  async getSessionTitles(sessionIds: string[]) {
    return window.api.getSessionTitles(sessionIds)
  }
}
