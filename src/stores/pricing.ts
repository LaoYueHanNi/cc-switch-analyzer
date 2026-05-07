import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { PricingData, TimePricingRule } from '@/types/pricing'

// 定价状态管理
export const usePricingStore = defineStore('pricing', () => {
  const pricingData = ref<PricingData[]>([])
  const timeRules = ref<TimePricingRule[]>([])

  return {
    pricingData,
    timeRules
  }
})
