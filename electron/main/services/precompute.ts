import type { DailyTrendRow, ProviderModelToken, SessionBreakdown, SessionModelToken, SessionRequestToken } from './external-db'
import type { PricingEngine, TokenDimensions } from './pricing-engine'
import { dateStrToEpoch } from '../utils/format'

// 预计算结果类型
export interface PrecomputedResult {
  modelCosts: Record<string, number>                          // 每个模型的总费用
  modelCostBreakdown: Record<string, [number, number, number, number]>  // [input, output, cacheRead, cacheCreation]
  providerCosts: Record<string, number>                       // 每个供应商的总费用
  dayCostMap: Record<string, number>                          // 每天的总费用
  dayRequestsMap: Record<string, number>                      // 每天的请求数
  dayInputTokens: Record<string, number>                      // 每天的输入 Token 数
  dayOutputTokens: Record<string, number>                     // 每天的输出 Token 数
  dayLatencySum: Record<string, number>                       // 每天的延迟总和
  dayLatencyCount: Record<string, number>                     // 每天的延迟样本数
  dailyByModel: Record<string, DailyTrendRow[]>               // 每个模型的每日数据
}

// 一次遍历 dailyTrend + providerModelTokens，产出所有预计算结果
export function precomputeCosts(
  dailyTrend: DailyTrendRow[],
  providerModelTokens: ProviderModelToken[],
  ps: PricingEngine
): PrecomputedResult {
  const modelCosts: Record<string, number> = {}
  const modelCostBreakdown: Record<string, [number, number, number, number]> = {}
  const providerCosts: Record<string, number> = {}
  const dayCostMap: Record<string, number> = {}
  const dayRequestsMap: Record<string, number> = {}
  const dayInputTokens: Record<string, number> = {}
  const dayOutputTokens: Record<string, number> = {}
  const dayLatencySum: Record<string, number> = {}
  const dayLatencyCount: Record<string, number> = {}
  const dailyByModel: Record<string, DailyTrendRow[]> = {}

  for (const row of dailyTrend) {
    const { day, model, requests, inputTokens, outputTokens, cacheRead, cacheCreation, avgLatency } = row

    // 成本计算（使用时间感知定价）
    const epoch = dateStrToEpoch(day)
    const pAt = ps.getPricingAt(model, epoch)
    let dayCost = 0
    let dayCostBreakdown: [number, number, number, number] = [0, 0, 0, 0]

    if (pAt) {
      const tokens: TokenDimensions = {
        input: inputTokens,
        output: outputTokens,
        cacheRead: cacheRead,
        cacheCreation: cacheCreation
      }
      dayCostBreakdown = ps.calculateCostBreakdown(pAt, tokens)
      dayCost = dayCostBreakdown[0] + dayCostBreakdown[1] + dayCostBreakdown[2] + dayCostBreakdown[3]
    }

    // 累加模型费用
    if (!modelCosts[model]) modelCosts[model] = 0
    modelCosts[model] += dayCost

    // 累加模型费用分解
    if (!modelCostBreakdown[model]) modelCostBreakdown[model] = [0, 0, 0, 0]
    const mb = modelCostBreakdown[model]
    mb[0] += dayCostBreakdown[0]
    mb[1] += dayCostBreakdown[1]
    mb[2] += dayCostBreakdown[2]
    mb[3] += dayCostBreakdown[3]

    // 累加每日统计
    if (!dayCostMap[day]) dayCostMap[day] = 0
    dayCostMap[day] += dayCost

    if (!dayRequestsMap[day]) dayRequestsMap[day] = 0
    dayRequestsMap[day] += requests

    if (!dayInputTokens[day]) dayInputTokens[day] = 0
    dayInputTokens[day] += inputTokens

    if (!dayOutputTokens[day]) dayOutputTokens[day] = 0
    dayOutputTokens[day] += outputTokens

    if (!dayLatencySum[day]) dayLatencySum[day] = 0
    dayLatencySum[day] += avgLatency * requests

    if (!dayLatencyCount[day]) dayLatencyCount[day] = 0
    dayLatencyCount[day] += requests

    // 按模型分组每日数据
    if (!dailyByModel[model]) dailyByModel[model] = []
    dailyByModel[model].push(row)
  }

  // 映射供应商费用：按 Token 比例将模型费用分配给供应商
  if (providerModelTokens.length > 0) {
    // 计算每个模型的 Token 总量（跨供应商）
    const modelTotalTokensMap: Record<string, number> = {}
    for (const pmt of providerModelTokens) {
      const total = pmt.inputTokens + pmt.outputTokens + pmt.cacheRead + pmt.cacheCreation
      if (!modelTotalTokensMap[pmt.model]) modelTotalTokensMap[pmt.model] = 0
      modelTotalTokensMap[pmt.model] += total
    }
    // 按 Token 占比分配模型费用
    for (const pmt of providerModelTokens) {
      const modelCost = modelCosts[pmt.model] || 0
      if (modelCost <= 0) continue
      const pmtTokens = pmt.inputTokens + pmt.outputTokens + pmt.cacheRead + pmt.cacheCreation
      const totalTokens = modelTotalTokensMap[pmt.model] || 0
      if (totalTokens <= 0 || pmtTokens <= 0) continue
      if (!providerCosts[pmt.providerId]) providerCosts[pmt.providerId] = 0
      providerCosts[pmt.providerId] += modelCost * (pmtTokens / totalTokens)
    }
  }

  return {
    modelCosts,
    modelCostBreakdown,
    providerCosts,
    dayCostMap,
    dayRequestsMap,
    dayInputTokens,
    dayOutputTokens,
    dayLatencySum,
    dayLatencyCount,
    dailyByModel
  }
}

// 计算会话费用（使用请求级时间感知定价）
export function computeSessionCosts(
  sessionRequestTokens: SessionRequestToken[],
  ps: PricingEngine
): Record<string, number> {
  const sessionCosts: Record<string, number> = {}

  for (const req of sessionRequestTokens) {
    const contextSize = req.inputTokens + req.cacheRead
    const pAt = ps.getPricingAtWithContext(req.model, req.createdAt, contextSize)
    if (pAt) {
      const tokens: TokenDimensions = {
        input: req.inputTokens,
        output: req.outputTokens,
        cacheRead: req.cacheRead,
        cacheCreation: req.cacheCreation
      }
      const cost = ps.calculateCost(pAt, tokens)
      if (!sessionCosts[req.sessionId]) sessionCosts[req.sessionId] = 0
      sessionCosts[req.sessionId] += cost
    }
  }

  return sessionCosts
}

// 计算模型-会话费用分解（从请求级数据聚合，使用时间感知定价）
export function computeSessionModelCosts(
  sessionRequestTokens: SessionRequestToken[],
  sessionModelTokens: SessionModelToken[],
  ps: PricingEngine
): Record<string, Record<string, { cost: number; breakdown: [number, number, number, number]; tokens: TokenDimensions }>> {
  const result: Record<string, Record<string, { cost: number; breakdown: [number, number, number, number]; tokens: TokenDimensions }>> = {}

  // 先填入 Token 总量（从 sessionModelTokens 聚合数据）
  for (const smt of sessionModelTokens) {
    if (!result[smt.sessionId]) result[smt.sessionId] = {}
    result[smt.sessionId][smt.model] = {
      cost: 0,
      breakdown: [0, 0, 0, 0],
      tokens: { input: smt.inputTokens, output: smt.outputTokens, cacheRead: smt.cacheRead, cacheCreation: smt.cacheCreation }
    }
  }

  // 从请求级数据累加费用（使用时间感知定价）
  for (const req of sessionRequestTokens) {
    const contextSize = req.inputTokens + req.cacheRead
    const pAt = ps.getPricingAtWithContext(req.model, req.createdAt, contextSize)
    if (!pAt) continue

    const tokens: TokenDimensions = {
      input: req.inputTokens,
      output: req.outputTokens,
      cacheRead: req.cacheRead,
      cacheCreation: req.cacheCreation
    }
    const reqBreakdown = ps.calculateCostBreakdown(pAt, tokens)
    const reqCost = reqBreakdown[0] + reqBreakdown[1] + reqBreakdown[2] + reqBreakdown[3]

    if (!result[req.sessionId]?.[req.model]) continue
    const entry = result[req.sessionId][req.model]
    entry.cost += reqCost
    entry.breakdown[0] += reqBreakdown[0]
    entry.breakdown[1] += reqBreakdown[1]
    entry.breakdown[2] += reqBreakdown[2]
    entry.breakdown[3] += reqBreakdown[3]
  }

  return result
}
