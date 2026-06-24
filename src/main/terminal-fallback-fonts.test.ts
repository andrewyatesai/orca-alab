import { describe, expect, it, vi } from 'vitest'

// app.getLocale() is only called inside getTerminalFallbackFonts (not under test
// here); stub electron so importing the module doesn't require an Electron runtime.
vi.mock('electron', () => ({ app: { getLocale: () => '' } }))

import { cjkRegionFromLocale } from './terminal-fallback-fonts'

describe('cjkRegionFromLocale (Han-unification region selection)', () => {
  it('maps Japanese locales to ja', () => {
    expect(cjkRegionFromLocale('ja')).toBe('ja')
    expect(cjkRegionFromLocale('ja-JP')).toBe('ja')
    expect(cjkRegionFromLocale('ja_JP.UTF-8')).toBe('ja')
  })

  it('maps Korean locales to ko', () => {
    expect(cjkRegionFromLocale('ko')).toBe('ko')
    expect(cjkRegionFromLocale('ko-KR')).toBe('ko')
  })

  it('maps Traditional-Chinese regions to zh-Hant', () => {
    expect(cjkRegionFromLocale('zh-TW')).toBe('zh-Hant')
    expect(cjkRegionFromLocale('zh-HK')).toBe('zh-Hant')
    expect(cjkRegionFromLocale('zh-Hant')).toBe('zh-Hant')
  })

  it('maps Simplified-Chinese (and bare zh) to zh-Hans', () => {
    expect(cjkRegionFromLocale('zh')).toBe('zh-Hans')
    expect(cjkRegionFromLocale('zh-CN')).toBe('zh-Hans')
    expect(cjkRegionFromLocale('zh-Hans')).toBe('zh-Hans')
  })

  it('defaults non-CJK / unknown locales to zh-Hans (prior behaviour)', () => {
    expect(cjkRegionFromLocale('en-US')).toBe('zh-Hans')
    expect(cjkRegionFromLocale('')).toBe('zh-Hans')
    expect(cjkRegionFromLocale('de')).toBe('zh-Hans')
  })
})
