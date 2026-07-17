// Fork adaptation of upstream #8810: upstream repairs xterm's internal
// `_isThirdLevelShift`; the fork's aterm key path applies the same predicates
// in aterm-key-encoding's AltGr gate, so the install/xterm-internal cases are
// replaced by engine-handoff cases (rescued chords are encoded by the ENGINE
// for whatever protocol the app negotiated, never by Orca).
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  isGenuineWindowsCtrlAltChord,
  shouldRepairWindowsCtrlAltChords
} from './terminal-windows-ctrl-alt-chord-classification'
import {
  ATERM_KEY_EVENT_PRESS,
  ATERM_KEY_MOD_ALT,
  ATERM_KEY_MOD_CTRL,
  encodeKeyEventToBytes
} from './aterm/aterm-key-encoding'

const WINDOWS_ELECTRON_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) ' +
  'orca/1.0.0 Chrome/126.0.0.0 Electron/31.0.0 Safari/537.36'
const WINDOWS_FIREFOX_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:127.0) Gecko/20100101 Firefox/127.0'
const MAC_ELECTRON_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) ' +
  'orca/1.0.0 Chrome/126.0.0.0 Electron/31.0.0 Safari/537.36'

type ClassificationEvent = {
  type: string
  key: string
  code: string
  ctrlKey: boolean
  altKey: boolean
  metaKey: boolean
  shiftKey: boolean
  repeat: boolean
  getModifierState?: (keyArg: string) => boolean
}

function chord(overrides: Partial<ClassificationEvent> = {}): ClassificationEvent {
  return {
    type: 'keydown',
    key: '2',
    code: 'Digit2',
    ctrlKey: true,
    altKey: true,
    metaKey: false,
    shiftKey: false,
    repeat: false,
    getModifierState: () => false,
    ...overrides
  }
}

describe('isGenuineWindowsCtrlAltChord', () => {
  it('accepts Ctrl+Alt chords whose AltGraph state is false', () => {
    expect(isGenuineWindowsCtrlAltChord(chord())).toBe(true)
    expect(isGenuineWindowsCtrlAltChord(chord({ shiftKey: true }))).toBe(true)
    // Synthetic events without getModifierState cannot be AltGr composition.
    expect(isGenuineWindowsCtrlAltChord(chord({ getModifierState: undefined }))).toBe(true)
  })

  it('rejects AltGr composition and non-Ctrl+Alt chords', () => {
    expect(
      isGenuineWindowsCtrlAltChord(chord({ getModifierState: (key) => key === 'AltGraph' }))
    ).toBe(false)
    expect(isGenuineWindowsCtrlAltChord(chord({ metaKey: true }))).toBe(false)
    expect(isGenuineWindowsCtrlAltChord(chord({ altKey: false }))).toBe(false)
    expect(isGenuineWindowsCtrlAltChord(chord({ ctrlKey: false }))).toBe(false)
  })
})

describe('shouldRepairWindowsCtrlAltChords', () => {
  it('repairs only Windows Chromium clients', () => {
    expect(shouldRepairWindowsCtrlAltChords(WINDOWS_ELECTRON_UA)).toBe(true)
    // Why: Firefox does not rewrite composing Ctrl+Alt presses to AltGraph, so
    // a false AltGraph state there does not prove the chord is genuine.
    expect(shouldRepairWindowsCtrlAltChords(WINDOWS_FIREFOX_UA)).toBe(false)
    expect(shouldRepairWindowsCtrlAltChords(MAC_ELECTRON_UA)).toBe(false)
  })
})

describe('aterm AltGr gate rescues genuine Windows Ctrl+Alt chords', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function encodeChord(overrides: Partial<ClassificationEvent> = {}) {
    const encode = vi.fn(() => new Uint8Array([0x1b, 0x32])) // ESC '2'
    const result = encodeKeyEventToBytes(chord(overrides) as unknown as KeyboardEvent, encode)
    return { encode, result }
  }

  it('hands a genuine Ctrl+Alt+digit chord to the ENGINE encoder on Windows Chromium', () => {
    vi.stubGlobal('navigator', { userAgent: WINDOWS_ELECTRON_UA })
    const { encode, result } = encodeChord()
    expect(encode).toHaveBeenCalledWith(
      '2',
      ATERM_KEY_MOD_CTRL | ATERM_KEY_MOD_ALT,
      ATERM_KEY_EVENT_PRESS,
      undefined
    )
    expect(result).toBe('\x1b2')
  })

  it('still leaves AltGr composition to the input path (AltGraph reported)', () => {
    vi.stubGlobal('navigator', { userAgent: WINDOWS_ELECTRON_UA })
    const { encode, result } = encodeChord({
      key: '@',
      getModifierState: (key) => key === 'AltGraph'
    })
    expect(encode).not.toHaveBeenCalled()
    expect(result).toBeNull()
  })

  it('keeps stock classification on clients without trustworthy AltGraph state', () => {
    vi.stubGlobal('navigator', { userAgent: WINDOWS_FIREFOX_UA })
    const { encode, result } = encodeChord()
    expect(encode).not.toHaveBeenCalled()
    expect(result).toBeNull()
  })
})
