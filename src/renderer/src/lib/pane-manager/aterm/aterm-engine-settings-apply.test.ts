/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { applyAtermEngineSettings } from './aterm-engine-settings-apply'
import type { AtermEffectsTarget } from './aterm-effects-settings'
import {
  shouldNoteAtermKeystroke,
  shouldNoteAtermMatrixRainActivity
} from './aterm-effects-activity-gate'
import {
  createAtermControllerOptionReaders,
  normalizeTerminalOpacity,
  type AtermControllerOptionReaders
} from './aterm-controller-option-readers'

// Proves the engine settings-apply seam drives EVERY live setter — including the
// per-cell minimum-contrast floor, the double-click word separators and the bg/cursor
// opacities — on both the initial apply and a settings-change reapply, that the
// per-pane-static kitty keyboard policy applies exactly once at construction, and that
// the option readers map terminalWordSeparator per the xterm-compat rule (unset/empty
// = engine default) and clamp the opacity settings to the engine's 0..=1 domain.

// The store is only consulted by the word-separator + opacity readers; stub just
// that surface.
let storeSettings:
  | {
      terminalWordSeparator?: string
      terminalBackgroundOpacity?: number
      terminalCursorOpacity?: number
    }
  | undefined
vi.mock('@/store', () => ({
  useAppStore: { getState: () => ({ settings: storeSettings }) }
}))

type RecordingTerm = {
  ligatures: boolean[]
  scrollback: number[]
  cursorStyles: number[]
  contrasts: number[]
  separators: (string | null | undefined)[]
  bgOpacities: number[]
  cursorOpacities: number[]
  kittyEnabled: boolean[]
  schemes: boolean[]
  sparkleMasters: boolean[]
  sparkleClasses: [boolean, boolean, boolean, boolean][]
  reducedMotions: boolean[]
  matrixRainMasters: boolean[]
  matrixRainProfiles: { hue: string; outputMaterial: boolean; seed: bigint }[]
  matrixRainReducedMotions: boolean[]
  glows: { enabled: boolean; style: string; color: number | undefined }[]
}

function makeTerm(): RecordingTerm & {
  liveCursorColor: number | undefined
} & Parameters<typeof applyAtermEngineSettings>[0]['term'] {
  const calls: RecordingTerm = {
    ligatures: [],
    scrollback: [],
    cursorStyles: [],
    contrasts: [],
    separators: [],
    bgOpacities: [],
    cursorOpacities: [],
    kittyEnabled: [],
    schemes: [],
    sparkleMasters: [],
    sparkleClasses: [],
    reducedMotions: [],
    matrixRainMasters: [],
    matrixRainProfiles: [],
    matrixRainReducedMotions: [],
    glows: []
  }
  const term = {
    ...calls,
    // The mock engine's live OSC 12 colour (tests mutate it between calls).
    liveCursorColor: undefined as number | undefined,
    get cursor_color(): number | undefined {
      return term.liveCursorColor
    },
    // Required by AtermEffectsTarget (window-chrome sizing); this fake carries no
    // windowChromeCapable marker, so chrome application stays a no-op here.
    cell_height: 16,
    set_sparkle_words_enabled: (on: boolean) => calls.sparkleMasters.push(on),
    set_sparkle_classes: (profanity: boolean, feline: boolean, orca: boolean, emphasis: boolean) =>
      calls.sparkleClasses.push([profanity, feline, orca, emphasis]),
    set_sparkle_reduced_motion: (on: boolean) => calls.reducedMotions.push(on),
    set_matrix_rain_enabled: (on: boolean) => calls.matrixRainMasters.push(on),
    set_matrix_rain: (...args: Parameters<AtermEffectsTarget['set_matrix_rain']>) =>
      calls.matrixRainProfiles.push({
        hue: args[6],
        outputMaterial: args[13],
        seed: args[14]
      }),
    set_matrix_rain_reduced_motion: (on: boolean) => calls.matrixRainReducedMotions.push(on),
    set_cursor_glow: (enabled: boolean, style: string, color: number | null | undefined) =>
      calls.glows.push({ enabled, style, color: color ?? undefined }),
    set_ligatures: (on: boolean) => calls.ligatures.push(on),
    set_scrollback_limit: (lines: number) => calls.scrollback.push(lines),
    set_default_cursor_style: (param: number) => calls.cursorStyles.push(param),
    set_minimum_contrast: (ratio: number) => calls.contrasts.push(ratio),
    set_word_separators: (separators?: string | null) => calls.separators.push(separators),
    set_background_opacity: (opacity: number) => calls.bgOpacities.push(opacity),
    set_cursor_opacity: (opacity: number) => calls.cursorOpacities.push(opacity),
    set_kitty_keyboard_enabled: (enabled: boolean) => calls.kittyEnabled.push(enabled),
    set_color_scheme: (dark: boolean) => calls.schemes.push(dark),
    take_response: () => undefined
  }
  return term
}

function makeReaders(
  overrides: Partial<AtermControllerOptionReaders>
): AtermControllerOptionReaders {
  return {
    getFontPx: () => 14,
    getLineHeight: () => 1,
    getFontFamily: () => undefined,
    getFontWeight: () => undefined,
    getLigatures: () => true,
    getScrollbackLines: () => 100_000,
    getCursorStyleParam: () => 1,
    getMinimumContrastRatio: () => 4.5,
    getWordSeparators: () => null,
    getBackgroundOpacity: () => 1,
    getCursorOpacity: () => 1,
    getKittyKeyboardEnabled: () => true,
    getEffectsConfig: () => ({
      sparkleWords: false,
      sparkleProfanity: true,
      sparkleFeline: true,
      sparkleOrca: true,
      sparkleEmphasis: true,
      matrixRain: false,
      cursorGlow: false,
      cursorGlowStyle: 'lumen' as const,
      reducedMotion: false
    }),
    getPredictiveEcho: () => 'adaptive',
    ...overrides
  }
}

describe('applyAtermEngineSettings', () => {
  it('applies minimum contrast + word separators on construction', () => {
    const term = makeTerm()
    applyAtermEngineSettings({
      term,
      readers: makeReaders({ getWordSeparators: () => ` ()[]{}'"` }),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw: () => undefined,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: () => undefined
    })
    expect(term.contrasts).toEqual([4.5])
    expect(term.separators).toEqual([` ()[]{}'"`])
  })

  it('applies background + cursor opacity on construction', () => {
    const term = makeTerm()
    applyAtermEngineSettings({
      term,
      readers: makeReaders({ getBackgroundOpacity: () => 0.85, getCursorOpacity: () => 0.4 }),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw: () => undefined,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: () => undefined
    })
    expect(term.bgOpacities).toEqual([0.85])
    expect(term.cursorOpacities).toEqual([0.4])
  })

  it('applies the kitty keyboard policy ONCE at construction (per-pane static)', () => {
    const term = makeTerm()
    const { reapply } = applyAtermEngineSettings({
      term,
      readers: makeReaders({ getKittyKeyboardEnabled: () => false }),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw: () => undefined,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: () => undefined
    })
    expect(term.kittyEnabled).toEqual([false])
    // A live settings change must NOT re-drive the static capability toggle.
    reapply()
    expect(term.kittyEnabled).toEqual([false])
  })

  it('re-reads + re-applies the live setters on reapply (live settings change)', () => {
    const term = makeTerm()
    let ratio = 4.5
    let seps: string | null = null
    let bgOpacity = 1
    let cursorOpacity = 1
    const scheduleDraw = vi.fn()
    const { reapply } = applyAtermEngineSettings({
      term,
      readers: makeReaders({
        getMinimumContrastRatio: () => ratio,
        getWordSeparators: () => seps,
        getBackgroundOpacity: () => bgOpacity,
        getCursorOpacity: () => cursorOpacity
      }),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: () => undefined
    })
    ratio = 1
    seps = ' ,;'
    bgOpacity = 0.7
    cursorOpacity = 0.5
    reapply()
    expect(term.contrasts).toEqual([4.5, 1])
    expect(term.separators).toEqual([null, ' ,;'])
    expect(term.bgOpacities).toEqual([1, 0.7])
    expect(term.cursorOpacities).toEqual([1, 0.5])
    // The existing setters still ride the same apply (regression guard).
    expect(term.ligatures).toEqual([true, true])
    expect(term.scrollback).toEqual([100_000, 100_000])
    expect(scheduleDraw).toHaveBeenCalled()
  })

  it('applies + live-reapplies the effects config (sparkle/classes/glow), gating glow off under reduced motion', () => {
    const term = makeTerm()
    let cfg: ReturnType<AtermControllerOptionReaders['getEffectsConfig']> = {
      sparkleWords: false,
      sparkleProfanity: true,
      sparkleFeline: true,
      sparkleOrca: true,
      sparkleEmphasis: true,
      matrixRain: false,
      cursorGlow: false,
      cursorGlowStyle: 'lumen',
      reducedMotion: false
    }
    const { reapply } = applyAtermEngineSettings({
      term,
      readers: makeReaders({ getEffectsConfig: () => cfg }),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw: () => undefined,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: () => undefined
    })
    // Everything-off construction: master off, glow off (engine byte-identical default).
    expect(term.sparkleMasters).toEqual([false])
    expect(term.matrixRainMasters).toEqual([false])
    expect(term.matrixRainProfiles).toEqual([{ hue: 'theme', outputMaterial: true, seed: 0n }])
    expect(term.glows).toEqual([{ enabled: false, style: 'lumen', color: undefined }])
    expect(shouldNoteAtermMatrixRainActivity(term)).toBe(false)
    expect(shouldNoteAtermKeystroke(term)).toBe(false)
    // Live enable: sparkle master + orca-only classes + rainbow glow.
    cfg = {
      ...cfg,
      sparkleWords: true,
      sparkleProfanity: false,
      sparkleFeline: false,
      sparkleEmphasis: false,
      matrixRain: true,
      cursorGlow: true,
      cursorGlowStyle: 'rainbow'
    }
    reapply()
    expect(term.sparkleMasters).toEqual([false, true])
    expect(term.matrixRainMasters).toEqual([false, true])
    expect(term.sparkleClasses.at(-1)).toEqual([false, false, true, false])
    expect(term.glows.at(-1)).toEqual({ enabled: true, style: 'rainbow', color: undefined })
    expect(shouldNoteAtermMatrixRainActivity(term)).toBe(true)
    expect(shouldNoteAtermKeystroke(term)).toBe(true)
    // OS reduce-motion: sparkle goes static in-engine; the pure-motion glow is
    // host-gated fully off.
    cfg = { ...cfg, reducedMotion: true }
    reapply()
    expect(term.reducedMotions.at(-1)).toBe(true)
    expect(term.matrixRainReducedMotions.at(-1)).toBe(true)
    expect(term.glows.at(-1)).toEqual({ enabled: false, style: 'rainbow', color: undefined })
    expect(shouldNoteAtermMatrixRainActivity(term)).toBe(false)
    expect(shouldNoteAtermKeystroke(term)).toBe(false)
  })

  it('folds the live OSC 12 cursor colour into the glow on apply and follows changes via syncCursorColor', () => {
    const term = makeTerm()
    const scheduleDraw = vi.fn()
    const glowOn = () => ({
      sparkleWords: false,
      sparkleProfanity: true,
      sparkleFeline: true,
      sparkleOrca: true,
      sparkleEmphasis: true,
      matrixRain: false,
      cursorGlow: true,
      cursorGlowStyle: 'lumen' as const,
      reducedMotion: false
    })
    term.liveCursorColor = 0x00ff00
    const { syncCursorColor } = applyAtermEngineSettings({
      term,
      readers: makeReaders({ getEffectsConfig: glowOn }),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: () => undefined
    })
    // Construction folds the live colour in.
    expect(term.glows.at(-1)).toEqual({ enabled: true, style: 'lumen', color: 0x00ff00 })
    // Unchanged colour: the per-chunk follow is a no-op.
    syncCursorColor()
    expect(term.glows).toHaveLength(1)
    // OSC 12 change → re-applied glow with the new colour + repaint.
    term.liveCursorColor = 0xff8800
    syncCursorColor()
    expect(term.glows.at(-1)).toEqual({ enabled: true, style: 'lumen', color: 0xff8800 })
    expect(scheduleDraw).toHaveBeenCalled()
    // OSC 112 reset → back to theme-derived (undefined colour).
    term.liveCursorColor = undefined
    syncCursorColor()
    expect(term.glows.at(-1)).toEqual({ enabled: true, style: 'lumen', color: undefined })
  })

  it('syncCursorColor records but does not touch the engine while the glow is off', () => {
    const term = makeTerm()
    const { syncCursorColor } = applyAtermEngineSettings({
      term,
      readers: makeReaders({}),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw: () => undefined,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: () => undefined
    })
    const glowsAfterApply = term.glows.length
    term.liveCursorColor = 0x123456
    syncCursorColor()
    expect(term.glows).toHaveLength(glowsAfterApply)
  })

  it('applies the predictive-echo mode on construction + live reapply', () => {
    const term = makeTerm()
    let mode: 'adaptive' | 'always' | 'off' = 'adaptive'
    const applied: string[] = []
    const { reapply } = applyAtermEngineSettings({
      term,
      readers: makeReaders({ getPredictiveEcho: () => mode }),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw: () => undefined,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: (m) => applied.push(m)
    })
    expect(applied).toEqual(['adaptive'])
    mode = 'off'
    reapply()
    expect(applied).toEqual(['adaptive', 'off'])
  })

  it('skips a reapply after dispose', () => {
    const term = makeTerm()
    let disposed = false
    const { reapply } = applyAtermEngineSettings({
      term,
      readers: makeReaders({}),
      inputSink: () => undefined,
      isDisposed: () => disposed,
      scheduleDraw: () => undefined,
      refreshCursorBlink: () => undefined,
      setPredictiveEcho: () => undefined
    })
    disposed = true
    reapply()
    expect(term.contrasts).toEqual([4.5])
    expect(term.separators).toEqual([null])
  })
})

describe('createAtermControllerOptionReaders (contrast + word separators)', () => {
  it('reads the fixed facade minimumContrastRatio default (4.5)', () => {
    const readers = createAtermControllerOptionReaders(undefined)
    expect(readers.getMinimumContrastRatio()).toBe(4.5)
  })

  it('maps unset/empty terminalWordSeparator to null (engine default), value verbatim', () => {
    const readers = createAtermControllerOptionReaders(undefined)
    storeSettings = undefined
    expect(readers.getWordSeparators()).toBeNull()
    // Empty string previously meant "xterm default" too — must clear, not "no separators".
    storeSettings = { terminalWordSeparator: '' }
    expect(readers.getWordSeparators()).toBeNull()
    storeSettings = { terminalWordSeparator: ' ,()' }
    expect(readers.getWordSeparators()).toBe(' ,()')
  })
})

describe('createAtermControllerOptionReaders (opacities + kitty policy)', () => {
  it('normalizes stored opacities: clamp 0..1, default 1 (unset / non-finite)', () => {
    expect(normalizeTerminalOpacity(undefined)).toBe(1)
    expect(normalizeTerminalOpacity(Number.NaN)).toBe(1)
    expect(normalizeTerminalOpacity(-0.5)).toBe(0)
    expect(normalizeTerminalOpacity(1.5)).toBe(1)
    expect(normalizeTerminalOpacity(0.42)).toBe(0.42)
  })

  it('reads the store live and clamps for both opacity readers', () => {
    const readers = createAtermControllerOptionReaders(undefined)
    storeSettings = undefined
    expect(readers.getBackgroundOpacity()).toBe(1)
    expect(readers.getCursorOpacity()).toBe(1)
    storeSettings = { terminalBackgroundOpacity: 0.6, terminalCursorOpacity: 2 }
    expect(readers.getBackgroundOpacity()).toBe(0.6)
    expect(readers.getCursorOpacity()).toBe(1)
    storeSettings = { terminalBackgroundOpacity: -1, terminalCursorOpacity: 0.25 }
    expect(readers.getBackgroundOpacity()).toBe(0)
    expect(readers.getCursorOpacity()).toBe(0.25)
  })

  it('kitty keyboard defaults to enabled; the pane policy callback can disable it', () => {
    expect(createAtermControllerOptionReaders(undefined).getKittyKeyboardEnabled()).toBe(true)
    const disabled = createAtermControllerOptionReaders({
      getKittyKeyboardEnabled: () => false
    })
    expect(disabled.getKittyKeyboardEnabled()).toBe(false)
  })
})
