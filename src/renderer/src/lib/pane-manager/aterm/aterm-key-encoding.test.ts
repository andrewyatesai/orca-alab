import { describe, expect, it, vi } from 'vitest'
import {
  ATERM_KEY_EVENT_PRESS,
  ATERM_KEY_EVENT_RELEASE,
  ATERM_KEY_EVENT_REPEAT,
  ATERM_KEY_MOD_ALT,
  ATERM_KEY_MOD_CTRL,
  ATERM_KEY_MOD_SHIFT,
  ATERM_KEYBOARD_MODE_REPORT_ALL_KEYS_AS_ESC,
  atermAppKeyProtocolNegotiated,
  encodeKeyEventToBytes
} from './aterm-key-encoding'

// The extraction layer only reads type + key/code + modifier flags, so a plain
// object (cast to KeyboardEvent) avoids needing a DOM environment.
function keyEvent(
  key: string,
  modifiers: Partial<
    Pick<KeyboardEvent, 'ctrlKey' | 'altKey' | 'metaKey' | 'shiftKey' | 'repeat' | 'code' | 'type'>
  > = {}
): KeyboardEvent {
  return {
    type: 'keydown',
    key,
    code: '',
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    shiftKey: false,
    repeat: false,
    ...modifiers
  } as KeyboardEvent
}

// Stand-in for the engine encoder: records the extracted tuple and returns
// fixed bytes so tests assert the DOM→engine handoff, not byte production
// (byte production is the engine's, proven by its own Rust suites).
function mockEncoder(bytes: Uint8Array | undefined = new Uint8Array([0x1b, 0x5b, 0x41])) {
  return vi.fn((_key: string, _mods: number, _eventType: number, _base?: string | null) => bytes)
}

describe('encodeKeyEventToBytes (engine handoff + platform gates)', () => {
  it('passes a named key with the SHIFT modifier bit to the engine', () => {
    const encode = mockEncoder(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x32, 0x41]))
    const result = encodeKeyEventToBytes(keyEvent('ArrowUp', { shiftKey: true }), encode)
    expect(encode).toHaveBeenCalledWith(
      'ArrowUp',
      ATERM_KEY_MOD_SHIFT,
      ATERM_KEY_EVENT_PRESS,
      undefined
    )
    expect(result).toBe('\x1b[1;2A')
  })

  it('decodes the engine bytes as UTF-8', () => {
    const encode = mockEncoder(new Uint8Array([0x1b, 0xc3, 0xa5])) // ESC 'å'
    expect(encodeKeyEventToBytes(keyEvent('Enter'), encode)).toBe('\x1bå')
  })

  it('marks a repeated press as event_type REPEAT', () => {
    const encode = mockEncoder()
    encodeKeyEventToBytes(keyEvent('ArrowDown', { repeat: true }), encode)
    expect(encode).toHaveBeenCalledWith('ArrowDown', 0, ATERM_KEY_EVENT_REPEAT, undefined)
  })

  it('passes Ctrl chords on printables to the engine with the CTRL bit', () => {
    const encode = mockEncoder(new Uint8Array([0x03]))
    expect(encodeKeyEventToBytes(keyEvent('c', { ctrlKey: true }), encode)).toBe('\x03')
    expect(encode).toHaveBeenCalledWith('c', ATERM_KEY_MOD_CTRL, ATERM_KEY_EVENT_PRESS, undefined)
  })

  it('derives base_layout_key from event.code when the layout remaps the letter', () => {
    const encode = mockEncoder()
    encodeKeyEventToBytes(keyEvent('ф', { ctrlKey: true, code: 'KeyA' }), encode)
    expect(encode).toHaveBeenCalledWith('ф', ATERM_KEY_MOD_CTRL, ATERM_KEY_EVENT_PRESS, 'a')
    // Redundant base (same letter) is omitted so the engine never reports noise.
    encodeKeyEventToBytes(keyEvent('a', { ctrlKey: true, code: 'KeyA' }), encode)
    expect(encode).toHaveBeenLastCalledWith(
      'a',
      ATERM_KEY_MOD_CTRL,
      ATERM_KEY_EVENT_PRESS,
      undefined
    )
  })

  it('returns null for Cmd (metaKey) chords so the app owns the shortcut', () => {
    const encode = mockEncoder()
    expect(
      encodeKeyEventToBytes(keyEvent('c', { metaKey: true }), encode, { isMac: true })
    ).toBeNull()
    expect(
      encodeKeyEventToBytes(keyEvent('k', { metaKey: true }), encode, { isMac: true })
    ).toBeNull()
    expect(encode).not.toHaveBeenCalled()
  })

  it('returns null for a plain printable (it flows via the input path)', () => {
    const encode = mockEncoder()
    expect(encodeKeyEventToBytes(keyEvent('a'), encode)).toBeNull()
    // Shifted printables are still text ('A' arrives via the input event).
    expect(encodeKeyEventToBytes(keyEvent('A', { shiftKey: true }), encode)).toBeNull()
    expect(encode).not.toHaveBeenCalled()
  })

  it('returns null for Mac Option+printable when macOptionIsMeta is OFF (glyph composes)', () => {
    const encode = mockEncoder()
    expect(
      encodeKeyEventToBytes(keyEvent('å', { altKey: true }), encode, {
        isMac: true,
        macOptionIsMeta: false
      })
    ).toBeNull()
    expect(encode).not.toHaveBeenCalled()
  })

  it('passes Mac Option+printable to the engine with ALT when macOptionIsMeta is ON', () => {
    const encode = mockEncoder(new Uint8Array([0x1b, 0x61]))
    expect(
      encodeKeyEventToBytes(keyEvent('a', { altKey: true }), encode, {
        isMac: true,
        macOptionIsMeta: true
      })
    ).toBe('\x1ba')
    expect(encode).toHaveBeenCalledWith('a', ATERM_KEY_MOD_ALT, ATERM_KEY_EVENT_PRESS, undefined)
  })

  it('always treats non-Mac Alt+printable as a meta chord', () => {
    const encode = mockEncoder(new Uint8Array([0x1b, 0x62]))
    expect(encodeKeyEventToBytes(keyEvent('b', { altKey: true }), encode)).toBe('\x1bb')
  })

  it('lets AltGr-composed glyphs (Ctrl+Alt+non-letter) flow via the input path', () => {
    const encode = mockEncoder()
    // German layout AltGr+q composes '@' with ctrl+alt set — must stay text.
    expect(encodeKeyEventToBytes(keyEvent('@', { ctrlKey: true, altKey: true }), encode)).toBeNull()
    expect(encode).not.toHaveBeenCalled()
    // Ctrl+Alt+letter is a real meta-control chord and reaches the engine.
    encodeKeyEventToBytes(keyEvent('a', { ctrlKey: true, altKey: true }), encode)
    expect(encode).toHaveBeenCalledWith(
      'a',
      ATERM_KEY_MOD_ALT | ATERM_KEY_MOD_CTRL,
      ATERM_KEY_EVENT_PRESS,
      undefined
    )
  })

  it('marks a keyup as event_type RELEASE and passes it to the engine', () => {
    // kitty REPORT_EVENT_TYPES: releases encode as CSI-u with event-type :3.
    const encode = mockEncoder(
      new Uint8Array([0x1b, 0x5b, 0x39, 0x37, 0x3b, 0x31, 0x3a, 0x33, 0x75])
    )
    const result = encodeKeyEventToBytes(keyEvent('a', { type: 'keyup' }), encode)
    expect(encode).toHaveBeenCalledWith('a', 0, ATERM_KEY_EVENT_RELEASE, undefined)
    expect(result).toBe('\x1b[97;1:3u')
  })

  it('skips the printable input-path gates on release (keyups fire no input event)', () => {
    const encode = mockEncoder(new Uint8Array([0x1b, 0x5b, 0x75]))
    // A plain printable keydown is null (input path), but its release must
    // still reach the engine so kitty event-type apps see it.
    expect(encodeKeyEventToBytes(keyEvent('a'), encode)).toBeNull()
    expect(encode).not.toHaveBeenCalled()
    encodeKeyEventToBytes(keyEvent('a', { type: 'keyup' }), encode)
    expect(encode).toHaveBeenCalledWith('a', 0, ATERM_KEY_EVENT_RELEASE, undefined)
    // Mac Option+printable release also reaches the engine with macOptionIsMeta
    // OFF: the compose-glyph gate only exists to defer to the input path.
    encodeKeyEventToBytes(keyEvent('å', { type: 'keyup', altKey: true }), encode, {
      isMac: true,
      macOptionIsMeta: false
    })
    expect(encode).toHaveBeenLastCalledWith(
      'å',
      ATERM_KEY_MOD_ALT,
      ATERM_KEY_EVENT_RELEASE,
      undefined
    )
  })

  it('returns null for a legacy-mode release (engine emits nothing)', () => {
    const encode = vi.fn(() => new Uint8Array(0))
    expect(encodeKeyEventToBytes(keyEvent('Enter', { type: 'keyup' }), encode)).toBeNull()
    expect(encode).toHaveBeenCalledWith('Enter', 0, ATERM_KEY_EVENT_RELEASE, undefined)
  })

  it('returns null for a Cmd (metaKey) release so the app owns the shortcut end-to-end', () => {
    const encode = mockEncoder()
    expect(
      encodeKeyEventToBytes(keyEvent('c', { type: 'keyup', metaKey: true }), encode, {
        isMac: true
      })
    ).toBeNull()
    expect(encode).not.toHaveBeenCalled()
  })

  it('returns null when the engine has no encoding for the key', () => {
    const encode = vi.fn(() => undefined)
    expect(encodeKeyEventToBytes(keyEvent('Shift', { shiftKey: true }), encode)).toBeNull()
    const empty = mockEncoder(new Uint8Array(0))
    expect(encodeKeyEventToBytes(keyEvent('Enter'), empty)).toBeNull()
  })
})

describe('atermAppKeyProtocolNegotiated', () => {
  it('is false for the legacy mode bits (DECCKM/keypad/backarrow only)', () => {
    expect(atermAppKeyProtocolNegotiated(0)).toBe(false)
    // APP_CURSOR (1<<2) | APP_KEYPAD (1<<3) | BACKARROW_SENDS_BS (1<<11).
    expect(atermAppKeyProtocolNegotiated(0x4 | 0x8 | 0x800)).toBe(false)
  })

  it('is true once kitty or modifyOtherKeys flags are pushed', () => {
    expect(atermAppKeyProtocolNegotiated(0x1)).toBe(true) // kitty disambiguate
    expect(atermAppKeyProtocolNegotiated(0x20)).toBe(true) // modifyOtherKeys L2
    expect(atermAppKeyProtocolNegotiated(0x100)).toBe(true) // report-all-keys
  })
})

describe('kitty REPORT_ALL_KEYS_AS_ESC printable routing', () => {
  const reportAll = { getKeyboardModeBits: () => ATERM_KEYBOARD_MODE_REPORT_ALL_KEYS_AS_ESC }
  const legacyBits = { getKeyboardModeBits: () => 0x1 | 0x2 } // kitty, but no flag 8

  it('routes plain printable presses to the engine (text will not be sent)', () => {
    const encode = mockEncoder(new Uint8Array([0x1b, 0x5b, 0x39, 0x37, 0x75])) // ESC[97u
    expect(encodeKeyEventToBytes(keyEvent('a'), encode, reportAll)).toBe('\x1b[97u')
    expect(encode).toHaveBeenCalledWith('a', 0, ATERM_KEY_EVENT_PRESS, undefined)
    // Repeats too — the engine owns downgrade/report semantics.
    encodeKeyEventToBytes(keyEvent('a', { repeat: true }), encode, reportAll)
    expect(encode).toHaveBeenLastCalledWith('a', 0, ATERM_KEY_EVENT_REPEAT, undefined)
    // Shifted printables as well ('A' must not sneak out via the input path).
    encodeKeyEventToBytes(keyEvent('A', { shiftKey: true }), encode, reportAll)
    expect(encode).toHaveBeenLastCalledWith(
      'A',
      ATERM_KEY_MOD_SHIFT,
      ATERM_KEY_EVENT_PRESS,
      undefined
    )
  })

  it('keeps the input-path gates when report-all is NOT negotiated', () => {
    const encode = mockEncoder()
    expect(encodeKeyEventToBytes(keyEvent('a'), encode, legacyBits)).toBeNull()
    expect(encode).not.toHaveBeenCalled()
  })

  it('falls back to the text path when the engine returns nothing (keys never go dead)', () => {
    const encode = vi.fn(() => new Uint8Array(0))
    expect(encodeKeyEventToBytes(keyEvent('a'), encode, reportAll)).toBeNull()
  })

  it('keeps the metaKey firewall: Cmd chords stay app-domain even under report-all', () => {
    const encode = mockEncoder()
    expect(
      encodeKeyEventToBytes(keyEvent('c', { metaKey: true }), encode, {
        isMac: true,
        ...reportAll
      })
    ).toBeNull()
    expect(encode).not.toHaveBeenCalled()
  })
})
