import { describe, expect, it } from 'vitest'
import {
  fontStyleWeight,
  selectTerminalFontFaces,
  type FontFaceCandidate
} from './terminal-font-face-selection'

// Candidate lists mirror what the platform discovery paths produce (macOS
// system_profiler typefaces, Linux fc-list styles, the Windows Fonts registry),
// so these pin the honest weight→face mapping without touching real OS fonts.

function face(style: string, path: string): FontFaceCandidate {
  return { style, path }
}

const JETBRAINS: FontFaceCandidate[] = [
  face('Regular', '/fonts/JetBrainsMono-Regular.ttf'),
  face('Bold', '/fonts/JetBrainsMono-Bold.ttf'),
  face('Italic', '/fonts/JetBrainsMono-Italic.ttf'),
  face('Bold Italic', '/fonts/JetBrainsMono-BoldItalic.ttf')
]

describe('fontStyleWeight', () => {
  it('maps named styles to weights, tolerating spacing/hyphen variants', () => {
    expect(fontStyleWeight('Regular')).toBe(400)
    expect(fontStyleWeight('Medium')).toBe(500)
    expect(fontStyleWeight('SemiBold')).toBe(600)
    expect(fontStyleWeight('Semi Bold')).toBe(600)
    expect(fontStyleWeight('extra-bold')).toBe(800)
    expect(fontStyleWeight('Black')).toBe(900)
    expect(fontStyleWeight('')).toBe(400)
  })

  it('rejects italic, width variants, and unrecognized style names', () => {
    expect(fontStyleWeight('Italic')).toBeNull()
    expect(fontStyleWeight('Bold Italic')).toBeNull()
    expect(fontStyleWeight('Condensed Bold')).toBeNull()
    // A sibling family suffix ("JetBrains Mono NL" leaking through a prefix
    // match) must not be mistaken for a weight of the requested family.
    expect(fontStyleWeight('NL Bold')).toBeNull()
    expect(fontStyleWeight('W3')).toBeNull()
  })
})

describe('selectTerminalFontFaces', () => {
  it('picks Regular + the real Bold face at the default weight', () => {
    const { primary, bold } = selectTerminalFontFaces(JETBRAINS, 500)
    expect(primary?.style).toBe('Regular')
    expect(bold?.style).toBe('Bold')
  })

  it('resolves the closest named style for a non-default weight', () => {
    const family = [
      face('Light', '/fonts/X-Light.ttf'),
      face('Regular', '/fonts/X-Regular.ttf'),
      face('Medium', '/fonts/X-Medium.ttf'),
      face('Bold', '/fonts/X-Bold.ttf')
    ]
    expect(selectTerminalFontFaces(family, 300).primary?.style).toBe('Light')
    expect(selectTerminalFontFaces(family, 500).primary?.style).toBe('Medium')
    expect(selectTerminalFontFaces(family, 700).primary?.style).toBe('Bold')
  })

  it('derives the bold face from the bold weight, allowing SemiBold when closest', () => {
    const family = [
      face('Regular', '/fonts/X-Regular.ttf'),
      face('SemiBold', '/fonts/X-SemiBold.ttf'),
      face('Black', '/fonts/X-Black.ttf')
    ]
    // fontWeightBold for 400 is 700: SemiBold (600) is closer than Black (900).
    expect(selectTerminalFontFaces(family, 400).bold?.style).toBe('SemiBold')
  })

  it('returns no bold face when the family ships nothing heavier than the primary', () => {
    expect(selectTerminalFontFaces([face('Regular', '/fonts/X-Regular.ttf')], 500).bold).toBeNull()
    // Primary already resolved to the heaviest face → synthetic embolden.
    const { primary, bold } = selectTerminalFontFaces(JETBRAINS, 800)
    expect(primary?.style).toBe('Bold')
    expect(bold).toBeNull()
  })

  it('never returns a bold face sharing the primary file (single-face engine loader)', () => {
    // macOS .ttc collections list every style under one path (e.g. PingFang.ttc).
    const collection = [
      face('Regular', '/System/Library/Fonts/X.ttc'),
      face('Bold', '/System/Library/Fonts/X.ttc')
    ]
    const { primary, bold } = selectTerminalFontFaces(collection, 500)
    expect(primary?.path).toBe('/System/Library/Fonts/X.ttc')
    expect(bold).toBeNull()
  })

  it('ignores italic faces for both roles', () => {
    const italicOnlyBold = [
      face('Regular', '/fonts/X-Regular.ttf'),
      face('Bold Italic', '/fonts/X-BoldItalic.ttf')
    ]
    expect(selectTerminalFontFaces(italicOnlyBold, 500).bold).toBeNull()
  })

  it('prefers a single-face file over a .ttc at equal weight', () => {
    const family = [face('Regular', '/fonts/X.ttc'), face('Regular', '/fonts/X-Regular.otf')]
    expect(selectTerminalFontFaces(family, 500).primary?.path).toBe('/fonts/X-Regular.otf')
  })

  it('falls back to the first single-face file when no style name is recognizable', () => {
    const family = [face('W3', '/fonts/X.ttc'), face('W4', '/fonts/X-W4.otf')]
    const { primary, bold } = selectTerminalFontFaces(family, 500)
    expect(primary?.path).toBe('/fonts/X-W4.otf')
    expect(bold).toBeNull()
  })

  it('returns nulls for an empty candidate list', () => {
    expect(selectTerminalFontFaces([], 500)).toEqual({ primary: null, bold: null })
  })
})
