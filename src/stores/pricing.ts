import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { PricingData, PricingFamily } from '@/types/pricing'

// 定价状态管理
export const usePricingStore = defineStore('pricing', () => {
  const pricingData = ref<PricingData[]>([])
  const families = ref<PricingFamily[]>([])

  return {
    pricingData,
    families
  }
})
