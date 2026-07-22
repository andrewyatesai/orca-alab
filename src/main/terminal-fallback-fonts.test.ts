import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// app.getLocale() is only called inside getTerminalFallbackFonts (not under test
// here); stub electron so importing the module doesn't require an Electron runtime.
vi.mock('electron', () => ({ app: { getLocale: () => 'en-US' } }))

// fs/promises.readFile is the only filesystem touch; mock it so tests control
// exactly which candidate paths "exist" without depending on the host's fonts.
const readFileMock = vi.fn<(path: string) => Promise<Buffer>>()
vi.mock('fs/promises', () => ({ readFile: (path: string) => readFileMock(path) }))

// The user-stack half resolves through system-fonts' family→bytes seam; mock it
// so tests control which families exist and which file each resolves to.
const resolveFaceMock =
  vi.fn<
    (
      family: string,
      weight?: number
    ) => Promise<{
      primary: Uint8Array | null
      bold: Uint8Array | null
      primaryPath: string | null
    }>
  >()
vi.mock('./system-fonts', () => ({
  resolveTerminalFontFaceBytes: (family: string, weight?: number) => resolveFaceMock(family, weight)
}))

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

// PC-8367: the user-configured fallback stack (terminalFontFallbackFamilies)
// resolves through the family→bytes seam and precedes the CJK face.
describe('getTerminalFallbackFonts (user fallback stacks)', () => {
  let originalPlatform: PropertyDescriptor | undefined

  const MAC_CJK = '/System/Library/Fonts/PingFang.ttc'

  // Families that "exist": each maps to its resolved file path + a marker byte.
  function resolvableFamilies(table: Record<string, { path: string; byte: number }>): void {
    resolveFaceMock.mockImplementation(async (family: string) => {
      const found = table[family]
      if (!found) {
        return { primary: null, bold: null, primaryPath: null }
      }
      return { primary: new Uint8Array([found.byte]), bold: null, primaryPath: found.path }
    })
  }

  async function freshModule(): Promise<typeof import('./terminal-fallback-fonts')> {
    vi.resetModules()
    return import('./terminal-fallback-fonts')
  }

  beforeEach(() => {
    originalPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
  })

  afterEach(() => {
    if (originalPlatform) {
      Object.defineProperty(process, 'platform', originalPlatform)
    }
    readFileMock.mockReset()
    resolveFaceMock.mockReset()
  })

  it('user families resolve in order and land in `user` before the CJK face', async () => {
    existingPaths(MAC_CJK)
    resolvableFamilies({
      'Fira Code': { path: '/fonts/FiraCode.ttf', byte: 0xaa },
      Iosevka: { path: '/fonts/Iosevka.ttf', byte: 0xbb }
    })
    const mod = await freshModule()
    const fonts = await mod.getTerminalFallbackFonts(['text'], ['Fira Code', 'Iosevka'])
    expect(fonts.user.map((face) => face.family)).toEqual(['Fira Code', 'Iosevka'])
    expect([...fonts.user[0].bytes]).toEqual([0xaa])
    expect([...fonts.user[1].bytes]).toEqual([0xbb])
    // The OS half is untouched: the CJK face still rides its own field, after the stack.
    expect(fonts.cjk).toBeDefined()
    // Resolution uses weight 400 (the stack has no per-family weight knob).
    expect(resolveFaceMock).toHaveBeenCalledWith('Fira Code', 400)
  })

  it('an unresolvable family is skipped, later ones survive', async () => {
    existingPaths()
    resolvableFamilies({ Iosevka: { path: '/fonts/Iosevka.ttf', byte: 0xbb } })
    const mod = await freshModule()
    const fonts = await mod.getTerminalFallbackFonts(['text'], ['No Such Font', 'Iosevka'])
    expect(fonts.user.map((face) => face.family)).toEqual(['Iosevka'])
  })

  it('a user face de-dups against CJK/chain by resolved path', async () => {
    existingPaths(MAC_CJK, MAC_ARABIC)
    resolvableFamilies({
      // Resolves to the very files the OS half already ships (CJK + arabic chain).
      PingFang: { path: MAC_CJK, byte: 0x01 },
      'Geeza Pro': { path: MAC_ARABIC, byte: 0x02 },
      Iosevka: { path: '/fonts/Iosevka.ttf', byte: 0xbb }
    })
    const mod = await freshModule()
    const fonts = await mod.getTerminalFallbackFonts(['text'], ['PingFang', 'Geeza Pro', 'Iosevka'])
    expect(fonts.user.map((face) => face.family)).toEqual(['Iosevka'])
    expect(fonts.cjk).toBeDefined()
    expect(fonts.chain.map((entry) => entry.script)).toEqual(['arabic'])
  })

  it('the user-half memo misses when the family list changes and hits when it repeats', async () => {
    existingPaths()
    resolvableFamilies({
      A: { path: '/fonts/A.ttf', byte: 0x0a },
      B: { path: '/fonts/B.ttf', byte: 0x0b }
    })
    const mod = await freshModule()

    await mod.getTerminalFallbackFonts(['text'], ['A'])
    expect(resolveFaceMock).toHaveBeenCalledTimes(1)

    // Same list → memo hit, no re-resolution.
    await mod.getTerminalFallbackFonts(['text'], ['A'])
    expect(resolveFaceMock).toHaveBeenCalledTimes(1)

    // Changed list → memo miss, the new list resolves.
    const fonts = await mod.getTerminalFallbackFonts(['text'], ['A', 'B'])
    expect(resolveFaceMock).toHaveBeenCalledTimes(3)
    expect(fonts.user.map((face) => face.family)).toEqual(['A', 'B'])
  })

  it("an emoji-scoped read never resolves user families (they ride the 'text' class)", async () => {
    existingPaths()
    resolvableFamilies({ A: { path: '/fonts/A.ttf', byte: 0x0a } })
    const mod = await freshModule()
    const fonts = await mod.getTerminalFallbackFonts(['emoji'], ['A'])
    expect(fonts.user).toEqual([])
    expect(resolveFaceMock).not.toHaveBeenCalled()
  })
})
