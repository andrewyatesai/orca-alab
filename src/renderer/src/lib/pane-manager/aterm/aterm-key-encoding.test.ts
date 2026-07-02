import { describe, expect, it, vi } from 'vitest'
import {
  ATERM_KEY_EVENT_PRESS,
  ATERM_KEY_EVENT_REPEAT,
  ATERM_KEY_MOD_ALT,
  ATERM_KEY_MOD_CTRL,
  ATERM_KEY_MOD_SHIFT,
  atermAppKeyProtocolNegotiated,
  encodeKeyEventToBytes
} from './aterm-key-encoding'

// The extraction layer only reads key/code + modifier flags, so a plain object
// (cast to KeyboardEvent) avoids needing a DOM environment.
function keyEvent(
  key: string,
  modifiers: Partial<
    Pick<KeyboardEvent, 'ctrlKey' | 'altKey' | 'metaKey' | 'shiftKey' | 'repeat' | 'code'>
  > = {}
): KeyboardEvent {
  return {
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
