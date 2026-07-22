import { describe, expect, it } from 'vitest'
import {
  atermThemeColorsFromITheme,
  contrastRatio,
  enforceDefaultContrast
} from './aterm-theme-colors'

// WCAG AA body-text floor the seeded default fg must clear against the default bg
// (MINOR b) — matches xterm's minimumContrastRatio default.
const MIN_CONTRAST_RATIO = 4.5

describe('enforceDefaultContrast (MINOR b)', () => {
  it('lifts a low-contrast fg/bg pair to at least the WCAG AA floor', () => {
    // Light-grey fg on a white bg is well under 4.5:1 — must be darkened.
    const bg = 0xffffff
    const fg = 0xbbbbbb
    expect(contrastRatio(fg, bg)).toBeLessThan(MIN_CONTRAST_RATIO)
    const adjusted = enforceDefaultContrast(fg, bg)
    expect(contrastRatio(adjusted, bg)).toBeGreaterThanOrEqual(MIN_CONTRAST_RATIO)
  })

  it('darkens the fg (toward black) on a light bg', () => {
    const adjusted = enforceDefaultContrast(0xbbbbbb, 0xffffff)
    // Each channel must be no lighter than the original (pushed toward black).
    expect((adjusted >> 16) & 0xff).toBeLessThanOrEqual(0xbb)
  })

  it('lightens the fg (toward white) on a dark bg', () => {
    // A near-black fg on a near-black bg → must be lightened toward white.
    const bg = 0x111111
    const adjusted = enforceDefaultContrast(0x222222, bg)
    expect(contrastRatio(adjusted, bg)).toBeGreaterThanOrEqual(MIN_CONTRAST_RATIO)
    expect((adjusted >> 16) & 0xff).toBeGreaterThanOrEqual(0x22)
  })

  it('leaves an already-readable pair unchanged', () => {
    // Engine defaults (light grey on near-black) already clear the floor.
    const bg = 0x111318
    const fg = 0xd0d0d0
    expect(contrastRatio(fg, bg)).toBeGreaterThanOrEqual(MIN_CONTRAST_RATIO)
    expect(enforceDefaultContrast(fg, bg)).toBe(fg)
  })
})

describe('atermThemeColorsFromITheme', () => {
  // Why: settings persisted before the `bold` override was removed (#8595) can still
  // spread a stale `bold` key into the composed ITheme; the engine seed must ignore
  // it — the wasm surface has no bold-color input.
  it('ignores a stale bold key riding on a composed theme', () => {
    const legacyTheme = {
      background: '#101010',
      foreground: '#fafafa',
      red: '#ff0000',
      bold: '#ff00ff'
    }
    const colors = atermThemeColorsFromITheme(legacyTheme)
    expect(colors.palette).toEqual([{ index: 1, rgb: 0xff0000 }])
    expect(colors.bg).toBe(0x101010)
  })
})
