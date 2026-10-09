import { ref } from 'vue'

type TokenUnit = 'K' | 'M' | 'B'
const tokenMultipliers: Record<TokenUnit, number> = {
  K: 1000,
  M: 1_000_000,
  B: 1_000_000_000
}

// 实际 token 与显示值分开保存，外部带入的数据不经过显示值回算。
export function useTokenInput(initialTokens: number) {
  const value = ref<number | null>(0)
  const unit = ref<TokenUnit>('K')
  const tokens = ref(0)

  function setTokens(amount: number): void {
    tokens.value = Number.isFinite(amount) ? Math.max(0, amount) : 0
    unit.value = tokens.value >= 1_000_000_000 ? 'B' : tokens.value >= 1_000_000 ? 'M' : 'K'
    value.value = tokens.value / tokenMultipliers[unit.value]
  }

  function updateValue(amount: number | null): void {
    if (amount === null || !Number.isFinite(amount)) {
      value.value = null
      unit.value = 'K'
      tokens.value = 0
      return
    }

    value.value = Math.max(0, amount)
    tokens.value = value.value * tokenMultipliers[unit.value]
  }

  function commitValue(): void {
    if (value.value === null) return

    // 编辑完成后才进位，避免连续输入时把后续数字接到换算后的小数上。
    while (unit.value !== 'B' && value.value >= 1000) {
      unit.value = unit.value === 'K' ? 'M' : 'B'
      value.value = tokens.value / tokenMultipliers[unit.value]
    }
  }

  setTokens(initialTokens)

  return { value, unit, tokens, setTokens, updateValue, commitValue }
}
