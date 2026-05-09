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
  async deleteTimePricingRule(data: { modelId: string; startTime: number; endTime: number; id: number }) {
    return window.api.deleteTimePricingRule(data)
  },
  async refreshPricing() {
    return window.api.refreshPricing()
  },
  // 上下文定价档位
  async saveOverrideContextTier(data: { modelId: string; threshold: number; input: number; output: number; cacheRead: number; cacheCreation: number }) {
    return window.api.saveOverrideContextTier(data)
  },
  async deleteOverrideContextTier(data: { modelId: string; threshold: number }) {
    return window.api.deleteOverrideContextTier(data)
  },
  async saveTimeRuleContextTier(data: { modelId: string; startTime: number; endTime: number; threshold: number; input: number; output: number; cacheRead: number; cacheCreation: number }) {
    return window.api.saveTimeRuleContextTier(data)
  },
  async updateTimeRuleContextTier(data: { id: number; input: number; output: number; cacheRead: number; cacheCreation: number }) {
    return window.api.updateTimeRuleContextTier(data)
  },
  async deleteTimeRuleContextTier(id: number) {
    return window.api.deleteTimeRuleContextTier(id)
  },
  async fetchCloudPricing() {
    return window.api.fetchCloudPricing()
  },
  async addUserAlias(modelId: string, alias: string) {
    return window.api.addUserAlias(modelId, alias)
  },
  async removeUserAlias(modelId: string, alias: string) {
    return window.api.removeUserAlias(modelId, alias)
  },
  async getSessionTitles(sessionIds: string[]) {
    return window.api.getSessionTitles(sessionIds)
  }
}
