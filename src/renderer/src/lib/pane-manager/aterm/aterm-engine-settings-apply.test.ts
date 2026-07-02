/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { applyAtermEngineSettings } from './aterm-engine-settings-apply'
import {
  createAtermControllerOptionReaders,
  type AtermControllerOptionReaders
} from './aterm-controller-option-readers'

// Proves the engine settings-apply seam drives EVERY live setter — including the
// per-cell minimum-contrast floor and the double-click word separators — on both the
// initial apply and a settings-change reapply, and that the option readers map the
// terminalWordSeparator setting per the xterm-compat rule (unset/empty = engine default).

// The store is only consulted by the word-separator reader; stub just that surface.
let storeSettings: { terminalWordSeparator?: string } | undefined
vi.mock('@/store', () => ({
  useAppStore: { getState: () => ({ settings: storeSettings }) }
}))

type RecordingTerm = {
  ligatures: boolean[]
  scrollback: number[]
  cursorStyles: number[]
  contrasts: number[]
  separators: (string | null | undefined)[]
  schemes: boolean[]
}

function makeTerm(): RecordingTerm & Parameters<typeof applyAtermEngineSettings>[0]['term'] {
  const calls: RecordingTerm = {
    ligatures: [],
    scrollback: [],
    cursorStyles: [],
    contrasts: [],
    separators: [],
    schemes: []
  }
  return {
    ...calls,
    set_ligatures: (on) => calls.ligatures.push(on),
    set_scrollback_limit: (lines) => calls.scrollback.push(lines),
    set_default_cursor_style: (param) => calls.cursorStyles.push(param),
    set_minimum_contrast: (ratio) => calls.contrasts.push(ratio),
    set_word_separators: (separators) => calls.separators.push(separators),
    set_color_scheme: (dark) => calls.schemes.push(dark),
    take_response: () => undefined
  }
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
      refreshCursorBlink: () => undefined
    })
    expect(term.contrasts).toEqual([4.5])
    expect(term.separators).toEqual([` ()[]{}'"`])
  })

  it('re-reads + re-applies both setters on reapply (live settings change)', () => {
    const term = makeTerm()
    let ratio = 4.5
    let seps: string | null = null
    const scheduleDraw = vi.fn()
    const { reapply } = applyAtermEngineSettings({
      term,
      readers: makeReaders({
        getMinimumContrastRatio: () => ratio,
        getWordSeparators: () => seps
      }),
      inputSink: () => undefined,
      isDisposed: () => false,
      scheduleDraw,
      refreshCursorBlink: () => undefined
    })
    ratio = 1
    seps = ' ,;'
    reapply()
    expect(term.contrasts).toEqual([4.5, 1])
    expect(term.separators).toEqual([null, ' ,;'])
    // The existing setters still ride the same apply (regression guard).
    expect(term.ligatures).toEqual([true, true])
    expect(term.scrollback).toEqual([100_000, 100_000])
    expect(scheduleDraw).toHaveBeenCalled()
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
      refreshCursorBlink: () => undefined
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
