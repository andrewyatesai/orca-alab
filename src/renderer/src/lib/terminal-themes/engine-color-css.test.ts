import { describe, expect, it } from 'vitest'
import { engineColorToCss } from './engine-color-css'

describe('engineColorToCss', () => {
  it('formats engine 0x00RRGGBB seeds as CSS hex', () => {
    expect(engineColorToCss(0x0a0a0a)).toBe('#0a0a0a')
    expect(engineColorToCss(0xffffff)).toBe('#ffffff')
    expect(engineColorToCss(0x282c34)).toBe('#282c34')
  })

  it('zero-pads small values', () => {
    expect(engineColorToCss(0x000001)).toBe('#000001')
    expect(engineColorToCss(0)).toBe('#000000')
  })

  it('masks any stray bits above 24', () => {
    expect(engineColorToCss(0xff123456)).toBe('#123456')
  })
})
