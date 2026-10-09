import { describe, expect, it } from 'vitest'
import { useTokenInput } from '@/composables/useTokenInput'

describe('定价 Token 输入单位', () => {
  it('在 1000 K 边界完成编辑后进位为 M，同时保留实际数量', () => {
    const input = useTokenInput(0)
    input.updateValue(999.999)
    expect(input.unit.value).toBe('K')
    expect(input.tokens.value).toBe(999_999)

    input.updateValue(1000)
    expect(input.unit.value).toBe('K')
    expect(input.value.value).toBe(1000)
    expect(input.tokens.value).toBe(1_000_000)
    input.commitValue()
    expect(input.unit.value).toBe('M')
    expect(input.value.value).toBe(1)
    expect(input.tokens.value).toBe(1_000_000)
  })

  it('带入大用量以 M 显示，实际 token 不因显示换算丢失精度', () => {
    const input = useTokenInput(14_596_345)
    expect(input.unit.value).toBe('M')
    expect(input.value.value).toBe(14.596345)
    expect(input.tokens.value).toBe(14_596_345)

    input.setTokens(1_000_001)
    expect(input.value.value).toBe(1.000001)
    expect(input.tokens.value).toBe(1_000_001)

    input.setTokens(266_360)
    expect(input.unit.value).toBe('K')
    expect(input.value.value).toBe(266.36)
    expect(input.tokens.value).toBe(266_360)
  })

  it('进位后继续按 M 编辑小数，清空后重新按 K 输入', () => {
    const input = useTokenInput(0)
    input.updateValue(1500)
    input.commitValue()
    input.updateValue(0.5)
    expect(input.unit.value).toBe('M')
    expect(input.tokens.value).toBe(500_000)

    input.updateValue(null)
    expect(input.unit.value).toBe('K')
    expect(input.value.value).toBeNull()
    expect(input.tokens.value).toBe(0)

    input.updateValue(10)
    expect(input.unit.value).toBe('K')
    expect(input.tokens.value).toBe(10_000)
  })

  it('在 1000 M 边界完成编辑后进位为 B，B 下继续编辑和清空均按正确单位计算', () => {
    const input = useTokenInput(1_000_000)
    input.updateValue(999.999)
    expect(input.unit.value).toBe('M')
    expect(input.tokens.value).toBe(999_999_000)

    input.updateValue(1000)
    expect(input.unit.value).toBe('M')
    expect(input.value.value).toBe(1000)
    expect(input.tokens.value).toBe(1_000_000_000)
    input.commitValue()
    expect(input.unit.value).toBe('B')
    expect(input.value.value).toBe(1)
    expect(input.tokens.value).toBe(1_000_000_000)

    input.updateValue(0.5)
    expect(input.unit.value).toBe('B')
    expect(input.tokens.value).toBe(500_000_000)

    input.updateValue(null)
    expect(input.unit.value).toBe('K')
    expect(input.value.value).toBeNull()
    expect(input.tokens.value).toBe(0)
    input.updateValue(1)
    expect(input.tokens.value).toBe(1000)
  })

  it('在 K 下粘贴大数后完成编辑可跨过 M 显示 B，费用仍按原始数量计算', () => {
    const input = useTokenInput(0)
    input.updateValue(2_500_000)
    expect(input.unit.value).toBe('K')
    expect(input.tokens.value).toBe(2_500_000_000)
    input.commitValue()
    expect(input.unit.value).toBe('B')
    expect(input.value.value).toBe(2.5)
    expect(input.tokens.value).toBe(2_500_000_000)
    expect(input.tokens.value * 3 / 1_000_000).toBe(7500)
  })

  it.each([
    { unit: 'K', initialTokens: 0, multiplier: 1000, committedValue: 11.111111 },
    { unit: 'M', initialTokens: 1_000_000, multiplier: 1_000_000, committedValue: 11111.111 }
  ])('连续输入 1 时保持 $unit 和整数文本，完成编辑后再换算', ({ unit, initialTokens, multiplier, committedValue }) => {
    const input = useTokenInput(initialTokens)
    for (let length = 1; length <= 8; length++) {
      // 首次替换选中内容，之后在当前显示值后逐键追加，复现用户的输入方式。
      const nextText = length === 1 ? '1' : `${input.value.value}1`
      input.updateValue(Number(nextText))
      const expectedValue = Number('1'.repeat(length))
      expect(input.unit.value).toBe(unit)
      expect(input.value.value).toBe(expectedValue)
      expect(input.tokens.value).toBe(expectedValue * multiplier)
    }

    const originalTokens = input.tokens.value
    input.commitValue()
    expect(input.unit.value).toBe('B')
    expect(input.value.value).toBe(committedValue)
    expect(input.tokens.value).toBe(originalTokens)
    // Enter 后再失焦不会重复改变单位、显示值或费用使用的数量。
    input.commitValue()
    expect(input.value.value).toBe(committedValue)
    expect(input.tokens.value).toBe(originalTokens)
  })

  it('清空后立即恢复 K，完成编辑不会把空输入恢复为数字', () => {
    const input = useTokenInput(1_000_000_000)
    input.updateValue(null)
    expect(input.unit.value).toBe('K')
    expect(input.tokens.value).toBe(0)
    input.commitValue()
    expect(input.value.value).toBeNull()
    expect(input.unit.value).toBe('K')
  })

  it('带入 B 级用量保留全部 token 精度，并可带入较小用量恢复 K 或 M', () => {
    const input = useTokenInput(1_000_000_001)
    expect(input.unit.value).toBe('B')
    expect(input.value.value).toBe(1.000000001)
    expect(input.tokens.value).toBe(1_000_000_001)

    input.setTokens(999_999_999)
    expect(input.unit.value).toBe('M')
    expect(input.tokens.value).toBe(999_999_999)
    input.setTokens(999_999)
    expect(input.unit.value).toBe('K')
    expect(input.tokens.value).toBe(999_999)
  })

  it('四项各自进位与清空，不影响其他项的用量或费用', () => {
    const input = useTokenInput(266_360)
    const output = useTokenInput(50_299)
    const cacheRead = useTokenInput(14_596_345)
    const cacheCreation = useTokenInput(0)
    const cost = () => (
      input.tokens.value * 10
      + output.tokens.value * 30
      + cacheRead.tokens.value * 2
      + cacheCreation.tokens.value * 5
    ) / 1_000_000

    expect(input.unit.value).toBe('K')
    expect(output.unit.value).toBe('K')
    expect(cacheRead.unit.value).toBe('M')
    expect(cacheCreation.unit.value).toBe('K')
    expect(cost()).toBeCloseTo(33.36526, 8)

    cacheCreation.updateValue(1500)
    cacheCreation.commitValue()
    expect(cacheCreation.unit.value).toBe('M')
    expect(cost()).toBeCloseTo(40.86526, 8)
    cacheCreation.updateValue(1000)
    cacheCreation.commitValue()
    expect(cacheCreation.unit.value).toBe('B')
    expect(cost()).toBeCloseTo(5033.36526, 8)
    cacheRead.updateValue(null)
    expect(cacheRead.unit.value).toBe('K')
    expect(cacheCreation.unit.value).toBe('B')
    expect(cost()).toBeCloseTo(5004.17257, 8)
  })
})
