// Keyboard-to-PTY-byte encoding for the aterm in-page renderer's helper
// textarea. The default xterm path owns the full encoder; here we cover the keys
// that make a real shell/TUI usable. Returns null when the event is not a key we
// should send so the caller can leave it to the browser/app shortcuts (and not
// preventDefault).

// Named keys → fixed escape sequences. Function keys use the standard xterm
// encodings (F1–F4 as SS3, F5+ as CSI ~). Editing/navigation keys use the
// common CSI forms most shells and TUIs expect.
const NAMED_KEY_SEQUENCES: Record<string, string> = {
  Enter: '\r',
  Backspace: '\x7f',
  Tab: '\t',
  Escape: '\x1b',
  ArrowUp: '\x1b[A',
  ArrowDown: '\x1b[B',
  ArrowRight: '\x1b[C',
  ArrowLeft: '\x1b[D',
  Home: '\x1b[H',
  End: '\x1b[F',
  PageUp: '\x1b[5~',
  PageDown: '\x1b[6~',
  Insert: '\x1b[2~',
  Delete: '\x1b[3~',
  F1: '\x1bOP',
  F2: '\x1bOQ',
  F3: '\x1bOR',
  F4: '\x1bOS',
  F5: '\x1b[15~',
  F6: '\x1b[17~',
  F7: '\x1b[18~',
  F8: '\x1b[19~',
  F9: '\x1b[20~',
  F10: '\x1b[21~',
  F11: '\x1b[23~',
  F12: '\x1b[24~'
}

// Ctrl+<symbol> → C0 control byte (Ctrl+letters are derived arithmetically).
const CTRL_SYMBOL_BYTES: Record<string, string> = {
  '[': '\x1b',
  ']': '\x1d',
  '\\': '\x1c',
  '_': '\x1f',
  ' ': '\x00'
}

function encodeCtrlChord(key: string): string | null {
  // Ctrl+A..Z → 0x01..0x1a (case-insensitive); Ctrl+[ ] \ _ Space → C0 bytes.
  if (/^[a-zA-Z]$/.test(key)) {
    return String.fromCharCode(key.toUpperCase().charCodeAt(0) - 64)
  }
  return CTRL_SYMBOL_BYTES[key] ?? null
}

export function encodeKeyEventToBytes(event: KeyboardEvent): string | null {
  const { key } = event

  // Ctrl chords (no Alt/Meta) → control bytes. Ctrl+C is encoded here for the
  // no-selection case; the controller checks the copy-chord first when a
  // selection exists, so it never reaches here in that case.
  if (event.ctrlKey && !event.altKey && !event.metaKey) {
    return encodeCtrlChord(key)
  }

  // Tab is special-cased before the named table so Shift+Tab → CSI Z (back-tab).
  if (key === 'Tab') {
    return event.shiftKey ? '\x1b[Z' : '\t'
  }

  const named = NAMED_KEY_SEQUENCES[key]
  if (named) {
    return named
  }

  // Alt/Option+<printable> → ESC prefix (meta), the conventional terminal form.
  if (event.altKey && !event.ctrlKey && !event.metaKey && key.length === 1) {
    return `\x1b${key}`
  }

  // Plain printable single character (no Ctrl/Meta/Alt). UTF-8 is sent as-is.
  if (key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
    return key
  }

  // Pure modifier presses (Shift/Control/Alt/Meta) and everything unhandled.
  return null
}
