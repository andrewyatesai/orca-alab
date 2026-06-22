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

  it('encodes ArrowUp as the CSI A sequence', () => {
    expect(encodeKeyEventToBytes(keyEvent('ArrowUp'))).toBe('\x1b[A')
  })

  it('encodes F5 as its xterm CSI sequence', () => {
    expect(encodeKeyEventToBytes(keyEvent('F5'))).toBe('\x1b[15~')
  })

  it('encodes Alt+b with an ESC meta prefix', () => {
    expect(encodeKeyEventToBytes(keyEvent('b', { altKey: true }))).toBe('\x1bb')
  })

  it('passes a plain printable character through', () => {
    expect(encodeKeyEventToBytes(keyEvent('a'))).toBe('a')
  })

  it('encodes Shift+Tab as the back-tab sequence', () => {
    expect(encodeKeyEventToBytes(keyEvent('Tab', { shiftKey: true }))).toBe('\x1b[Z')
  })

  it('returns null for a bare Shift press so the browser/app keep the key', () => {
    expect(encodeKeyEventToBytes(keyEvent('Shift', { shiftKey: true }))).toBeNull()
  })
})
