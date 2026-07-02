import { describe, expect, it } from 'vitest'
import {
  accumulateWheelLines,
  resolveScrollbackWheelSensitivity,
  resolveTuiWheelMultiplier,
  WHEEL_DELTA_LINE,
  WHEEL_DELTA_PAGE,
  WHEEL_DELTA_PIXEL
} from './aterm-wheel-lines'

const base = {
  deltaMode: WHEEL_DELTA_PIXEL,
  dpr: 2,
  cellHeight: 32,
  rows: 24,
  sensitivity: 1,
  remainder: 0
}

describe('accumulateWheelLines', () => {
  it('converts pixel deltas to lines via dpr and cell height', () => {
    // 48 CSS px * dpr 2 = 96 device px / 32 px cells = 3 lines.
    expect(accumulateWheelLines({ ...base, deltaY: 48 })).toEqual({ lines: 3, remainder: 0 })
  })

  it('carries a fractional remainder across events instead of dropping it', () => {
    // 10 CSS px * 2 / 32 = 0.625 lines: below one line, nothing scrolls yet.
    const first = accumulateWheelLines({ ...base, deltaY: 10 })
    expect(first.lines).toBe(0)
    expect(first.remainder).toBeCloseTo(0.625)
    // The next event pushes the accumulated total past a whole line.
    const second = accumulateWheelLines({ ...base, deltaY: 10, remainder: first.remainder })
    expect(second.lines).toBe(1)
    expect(second.remainder).toBeCloseTo(0.25)
  })

  it('treats line-mode deltas as lines directly', () => {
    expect(accumulateWheelLines({ ...base, deltaMode: WHEEL_DELTA_LINE, deltaY: -3 })).toEqual({
      lines: -3,
      remainder: 0
    })
  })

  it('scales page-mode deltas by the viewport rows', () => {
    expect(accumulateWheelLines({ ...base, deltaMode: WHEEL_DELTA_PAGE, deltaY: 1 })).toEqual({
      lines: 24,
      remainder: 0
    })
  })

  it('applies the sensitivity multiplier to the line count', () => {
    // 3 raw lines * 1.5 = 4.5 → 4 whole lines, 0.5 carried.
    const result = accumulateWheelLines({ ...base, deltaY: 48, sensitivity: 1.5 })
    expect(result.lines).toBe(4)
    expect(result.remainder).toBeCloseTo(0.5)
  })

  it('keeps sign symmetry for wheel-up (negative) deltas', () => {
    const result = accumulateWheelLines({ ...base, deltaY: -48 })
    expect(result).toEqual({ lines: -3, remainder: 0 })
  })
})

describe('resolveScrollbackWheelSensitivity', () => {
  it('uses scrollSensitivity alone without the fast modifier', () => {
    expect(
      resolveScrollbackWheelSensitivity({
        altKey: false,
        scrollSensitivity: 1.15,
        fastScrollSensitivity: 5
      })
    ).toBeCloseTo(1.15)
  })

  it('multiplies in fastScrollSensitivity while Alt is held', () => {
    expect(
      resolveScrollbackWheelSensitivity({
        altKey: true,
        scrollSensitivity: 1.15,
        fastScrollSensitivity: 5
      })
    ).toBeCloseTo(5.75)
  })
})

describe('resolveTuiWheelMultiplier', () => {
  it('applies the multiplier to line-mode (notched wheel) events', () => {
    expect(resolveTuiWheelMultiplier({ deltaY: 1, deltaMode: WHEEL_DELTA_LINE }, 3)).toBe(3)
  })

  it('applies the multiplier to large pixel deltas (Chromium notch)', () => {
    expect(resolveTuiWheelMultiplier({ deltaY: 120, deltaMode: WHEEL_DELTA_PIXEL }, 3)).toBe(3)
  })

  it('leaves fine-grained trackpad pixel deltas unmultiplied', () => {
    expect(resolveTuiWheelMultiplier({ deltaY: 8, deltaMode: WHEEL_DELTA_PIXEL }, 3)).toBe(1)
  })
})
