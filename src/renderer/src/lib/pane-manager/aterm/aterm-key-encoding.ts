// Minimal keyboard-to-PTY-byte encoding for the aterm in-page renderer. Phase 0
// only needs the keys that make a shell usable; xterm.js owns the full encoder
// on the default path. Returns null when the event is not a handled key so the
// caller can leave it to the browser.

const ARROW_SEQUENCES: Record<string, string> = {
  ArrowUp: '\x1b[A',
  ArrowDown: '\x1b[B',
  ArrowRight: '\x1b[C',
  ArrowLeft: '\x1b[D'
}

export function encodeKeyEventToBytes(event: KeyboardEvent): string | null {
  const { key } = event

  // Ctrl+letter -> control char (Ctrl+C -> 0x03, Ctrl+A -> 0x01, ...).
  if (event.ctrlKey && !event.altKey && !event.metaKey && /^[a-zA-Z]$/.test(key)) {
    return String.fromCharCode(key.toUpperCase().charCodeAt(0) - 64)
  }

  switch (key) {
    case 'Enter':
      return '\r'
    case 'Backspace':
      return '\x7f'
    case 'Tab':
      return '\t'
    case 'Escape':
      return '\x1b'
    default:
      break
  }

  const arrow = ARROW_SEQUENCES[key]
  if (arrow) {
    return arrow
  }

  // Printable single characters (ignore modified chords we do not encode).
  if (key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
    return key
  }

  return null
}
