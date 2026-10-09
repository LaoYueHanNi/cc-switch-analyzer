import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { PricingData, PricingFamily, TokenDimensions } from '@/types/pricing'

// 定价状态管理
export const usePricingStore = defineStore('pricing', () => {
  const pricingData = ref<PricingData[]>([])
  const families = ref<PricingFamily[]>([])
  // 模型页到定价页的一次性 token 交接请求。
  const pendingTokenImport = ref<TokenDimensions | null>(null)

  function requestTokenImport(tokens: TokenDimensions): void {
    pendingTokenImport.value = { ...tokens }
  }

  function consumeTokenImport(): TokenDimensions | null {
    const tokens = pendingTokenImport.value
    pendingTokenImport.value = null
    return tokens
  }

  return {
    pricingData,
    families,
    pendingTokenImport,
    requestTokenImport,
    consumeTokenImport
  }
})
