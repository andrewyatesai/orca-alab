import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// app.getLocale() is only called inside getTerminalFallbackFonts (not under test
// here); stub electron so importing the module doesn't require an Electron runtime.
vi.mock('electron', () => ({ app: { getLocale: () => 'en-US' } }))

// fs/promises.readFile is the only filesystem touch; mock it so tests control
// exactly which candidate paths "exist" without depending on the host's fonts.
const readFileMock = vi.fn<(path: string) => Promise<Buffer>>()
vi.mock('fs/promises', () => ({ readFile: (path: string) => readFileMock(path) }))

import { cjkRegionFromLocale } from './terminal-fallback-fonts'
import type { TerminalFallbackFonts } from './terminal-fallback-fonts'

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

// macOS candidate paths from NON_LATIN_CANDIDATES, used to drive existence in
// tests without asserting any machine-specific absolute path back to callers.
const MAC_ARABIC = '/System/Library/Fonts/Supplemental/GeezaPro.ttc'
const MAC_HEBREW = '/System/Library/Fonts/Supplemental/Arial Hebrew.ttc'
const MAC_DEVANAGARI = '/System/Library/Fonts/Supplemental/DevanagariMT.ttc'
const MAC_THAI = '/System/Library/Fonts/Supplemental/Ayuthaya.ttf'
const MAC_BROAD = '/Library/Fonts/Arial Unicode.ttf'

// Mark the given set of paths as the only ones that "exist"; all other reads
// reject (missing file), mirroring readFile's ENOENT.
function existingPaths(...paths: string[]): void {
  const present = new Set(paths)
  readFileMock.mockImplementation(async (path: string) => {
    if (present.has(path)) {
      return Buffer.from([0x00, 0x01, 0x00, 0x00]) // non-empty sfnt-ish bytes
    }
    throw new Error(`ENOENT: ${path}`)
  })
}

// The module caches discovery at module scope; force darwin so only the macOS
// candidate lists matter (no fontconfig/execFile) and re-import per test for a
// clean cache.
async function freshDiscover(): Promise<TerminalFallbackFonts> {
  vi.resetModules()
  const mod = await import('./terminal-fallback-fonts')
  return mod.getTerminalFallbackFonts()
}

describe('getTerminalFallbackFonts (non-Latin chain discovery)', () => {
  let originalPlatform: PropertyDescriptor | undefined

  beforeEach(() => {
    originalPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
  })

  afterEach(() => {
    if (originalPlatform) {
      Object.defineProperty(process, 'platform', originalPlatform)
    }
    readFileMock.mockReset()
  })

  it('orders the chain Arabic→Hebrew→Devanagari→Thai when each script has a face', async () => {
    existingPaths(MAC_ARABIC, MAC_HEBREW, MAC_DEVANAGARI, MAC_THAI)
    const fonts = await freshDiscover()
    expect(fonts.chain.map((entry) => entry.script)).toEqual([
      'arabic',
      'hebrew',
      'devanagari',
      'thai'
    ])
    // Every entry carries real, non-empty bytes (never a placeholder).
    for (const entry of fonts.chain) {
      expect(entry.bytes.length).toBeGreaterThan(0)
    }
  })

  it('skips scripts whose candidates are all missing (existence-filtering)', async () => {
    // Only Hebrew + Thai faces exist; Arabic and Devanagari have no host face.
    existingPaths(MAC_HEBREW, MAC_THAI)
    const fonts = await freshDiscover()
    expect(fonts.chain.map((entry) => entry.script)).toEqual(['hebrew', 'thai'])
  })

  it('returns an empty chain when no non-Latin face exists', async () => {
    existingPaths() // nothing exists
    const fonts = await freshDiscover()
    expect(fonts.chain).toEqual([])
    expect(fonts.cjk).toBeUndefined()
    expect(fonts.emoji).toBeUndefined()
    expect(fonts.symbol).toBeUndefined()
  })

  it('discovers the monochrome symbol face when a candidate exists', async () => {
    // Only the macOS STIX Two Math symbol face exists — it populates `symbol`
    // (the set_symbol_font tier) independently of cjk/emoji/chain.
    existingPaths('/System/Library/Fonts/Supplemental/STIXTwoMath.otf')
    const fonts = await freshDiscover()
    expect(fonts.symbol?.length).toBeGreaterThan(0)
    expect(fonts.cjk).toBeUndefined()
    expect(fonts.emoji).toBeUndefined()
    expect(fonts.chain).toEqual([])
  })

  it('de-dups a broad-coverage face shared across scripts (ships it once)', async () => {
    // Only the broad catch-all exists. All five scripts resolve to it, but de-dup
    // by path means a single chain entry (the first script that hit it: arabic).
    existingPaths(MAC_BROAD)
    const fonts = await freshDiscover()
    expect(fonts.chain).toHaveLength(1)
    expect(fonts.chain[0].script).toBe('arabic')
  })

  it('surfaces the CJK region on the cjk entry and keeps CJK first', async () => {
    // ja locale (mocked electron getLocale='en-US' falls to zh-Hans, so drive the
    // region face directly): the SC generic PingFang exists + Arabic.
    existingPaths('/System/Library/Fonts/PingFang.ttc', MAC_ARABIC)
    const fonts = await freshDiscover()
    expect(fonts.cjk).toBeDefined()
    expect(fonts.cjk?.bytes.length).toBeGreaterThan(0)
    expect(fonts.cjk?.region).toBe('zh-Hans')
    // The chain is appended after CJK, not merged into it.
    expect(fonts.chain.map((entry) => entry.script)).toEqual(['arabic'])
  })

  it('keeps a distinct chain face when CJK resolves to a different file', async () => {
    // CJK (PingFang) and the broad catch-all (Arial Unicode) are different files,
    // so the CJK-vs-chain de-dup must NOT suppress the chain entry.
    existingPaths('/System/Library/Fonts/PingFang.ttc', MAC_BROAD)
    const fonts = await freshDiscover()
    expect(fonts.cjk?.bytes.length).toBeGreaterThan(0)
    // Arial Unicode is the unicode catch-all; arabic also lists it, so de-dup
    // ships it once under the first script that hit it (arabic).
    expect(fonts.chain.map((entry) => entry.script)).toEqual(['arabic'])
  })

  it("classes: ['text'] never reads the emoji face; ['emoji'] never reads the mono faces (E1)", async () => {
    const MAC_EMOJI = '/System/Library/Fonts/Apple Color Emoji.ttc'
    const MAC_CJK = '/System/Library/Fonts/PingFang.ttc'
    existingPaths(MAC_CJK, MAC_EMOJI, MAC_ARABIC)
    vi.resetModules()
    const mod = await import('./terminal-fallback-fonts')

    const text = await mod.getTerminalFallbackFonts(['text'])
    expect(text.cjk).toBeDefined()
    expect(text.chain.map((entry) => entry.script)).toEqual(['arabic'])
    expect(text.emoji, 'text-scoped read must not surface emoji').toBeUndefined()
    expect(
      readFileMock.mock.calls.some(([path]) => path === MAC_EMOJI),
      'the ~183MB emoji face must not be read for a text-class request'
    ).toBe(false)

    readFileMock.mockClear()
    const emoji = await mod.getTerminalFallbackFonts(['emoji'])
    expect(emoji.emoji).toBeDefined()
    expect(emoji.cjk).toBeUndefined()
    expect(emoji.chain).toEqual([])
    expect(
      readFileMock.mock.calls.some(([path]) => path === MAC_CJK),
      'the text class is cached from the first read — and never re-read for emoji'
    ).toBe(false)

    // Per-class caching: a repeat request reads nothing at all.
    readFileMock.mockClear()
    const both = await mod.getTerminalFallbackFonts()
    expect(both.cjk).toBeDefined()
    expect(both.emoji).toBeDefined()
    expect(readFileMock).not.toHaveBeenCalled()
  })
})
