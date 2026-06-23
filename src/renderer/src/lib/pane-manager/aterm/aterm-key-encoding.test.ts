import { describe, expect, it } from 'vitest'
import { encodeKeyEventToBytes } from './aterm-key-encoding'

// The encoder only reads key + modifier flags, so a plain object (cast to
// KeyboardEvent) avoids needing a DOM environment for these unit assertions.
function keyEvent(
  key: string,
  modifiers: Partial<Pick<KeyboardEvent, 'ctrlKey' | 'altKey' | 'metaKey' | 'shiftKey'>> = {}
): KeyboardEvent {
  return {
    key,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    shiftKey: false,
    ...modifiers
  } as KeyboardEvent
}

describe('encodeKeyEventToBytes', () => {
  it('encodes Enter as carriage return', () => {
    expect(encodeKeyEventToBytes(keyEvent('Enter'))).toBe('\r')
  })

  it('encodes Ctrl+C as the ^C control byte', () => {
    expect(encodeKeyEventToBytes(keyEvent('c', { ctrlKey: true }))).toBe('\x03')
  })

  it('encodes ArrowUp as the CSI A sequence in normal cursor mode', () => {
    expect(encodeKeyEventToBytes(keyEvent('ArrowUp'))).toBe('\x1b[A')
  })

  it('encodes arrows as SS3 in application-cursor mode (DECCKM)', () => {
    expect(encodeKeyEventToBytes(keyEvent('ArrowUp'), { appCursor: true })).toBe('\x1bOA')
    expect(encodeKeyEventToBytes(keyEvent('ArrowDown'), { appCursor: true })).toBe('\x1bOB')
    expect(encodeKeyEventToBytes(keyEvent('ArrowRight'), { appCursor: true })).toBe('\x1bOC')
    expect(encodeKeyEventToBytes(keyEvent('ArrowLeft'), { appCursor: true })).toBe('\x1bOD')
  })

  it('encodes arrows as CSI when app-cursor mode is off', () => {
    expect(encodeKeyEventToBytes(keyEvent('ArrowUp'), { appCursor: false })).toBe('\x1b[A')
    expect(encodeKeyEventToBytes(keyEvent('ArrowLeft'), { appCursor: false })).toBe('\x1b[D')
  })

  it('splits Home/End between SS3 (app-cursor) and CSI (normal)', () => {
    expect(encodeKeyEventToBytes(keyEvent('Home'))).toBe('\x1b[H')
    expect(encodeKeyEventToBytes(keyEvent('End'))).toBe('\x1b[F')
    expect(encodeKeyEventToBytes(keyEvent('Home'), { appCursor: true })).toBe('\x1bOH')
    expect(encodeKeyEventToBytes(keyEvent('End'), { appCursor: true })).toBe('\x1bOF')
  })

  it('encodes Alt+Ctrl+A as ESC + the ^A control byte', () => {
    expect(encodeKeyEventToBytes(keyEvent('a', { ctrlKey: true, altKey: true }))).toBe('\x1b\x01')
  })

  it('encodes Alt+Ctrl+C as ESC + the ^C control byte', () => {
    expect(encodeKeyEventToBytes(keyEvent('c', { ctrlKey: true, altKey: true }))).toBe('\x1b\x03')
  })

  it('returns null for Alt+Ctrl with a non-control key', () => {
    expect(encodeKeyEventToBytes(keyEvent('F13', { ctrlKey: true, altKey: true }))).toBeNull()
  })

  it('encodes F5 as its xterm CSI sequence', () => {
    expect(encodeKeyEventToBytes(keyEvent('F5'))).toBe('\x1b[15~')
  })

  it('encodes Alt+b with an ESC meta prefix', () => {
    expect(encodeKeyEventToBytes(keyEvent('b', { altKey: true }))).toBe('\x1bb')
  })

  it('returns null for a plain printable character (sent via the input path)', () => {
    // Under the input model, plain typed chars flow through the textarea
    // 'input'/IME path, not keydown — so the encoder must not emit them here
    // (otherwise the character is double-sent).
    expect(encodeKeyEventToBytes(keyEvent('a'))).toBeNull()
  })

  it('encodes Shift+Tab as the back-tab sequence', () => {
    expect(encodeKeyEventToBytes(keyEvent('Tab', { shiftKey: true }))).toBe('\x1b[Z')
  })

  it('returns null for a bare Shift press so the browser/app keep the key', () => {
    expect(encodeKeyEventToBytes(keyEvent('Shift', { shiftKey: true }))).toBeNull()
  })
})
