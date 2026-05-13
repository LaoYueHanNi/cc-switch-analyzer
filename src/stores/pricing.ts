import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { PricingData } from '@/types/pricing'

// 定价状态管理
export const usePricingStore = defineStore('pricing', () => {
  const pricingData = ref<PricingData[]>([])

  return {
    pricingData
  }
})
