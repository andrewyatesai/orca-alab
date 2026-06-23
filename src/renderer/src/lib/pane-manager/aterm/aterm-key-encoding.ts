// Keyboard-to-PTY-byte encoding for the aterm in-page renderer's helper
// textarea. The default xterm path owns the full encoder; here we cover the keys
// that make a real shell/TUI usable. Returns null when the event is not a key we
// should send so the caller can leave it to the browser/app shortcuts (and not
// preventDefault).
//
// Input model (mirrors xterm): this encoder handles ONLY non-text keys (Enter,
// Tab, Backspace, Escape, arrows, editing/nav, F-keys, Ctrl/Alt chords). Plain
// printable characters return null and flow through the textarea 'input'/IME path
// instead, so they are never double-sent. See aterm-pane-renderer.ts.

/** Per-press encoder options. `appCursor` reflects DECCKM (the engine's
 *  `is_app_cursor_mode`): when set, arrows + Home/End use the SS3 (ESC O) form
 *  full-screen apps (vi, less, readline) expect instead of the CSI (ESC [) form. */
export type AtermKeyEncodingOptions = {
  appCursor?: boolean
}

// Named keys → fixed escape sequences. Function keys use the standard xterm
// encodings (F1–F4 as SS3, F5+ as CSI ~). Editing/navigation keys use the
// common CSI forms most shells and TUIs expect. Cursor keys (arrows, Home/End)
// are mode-dependent and resolved in encodeCursorKey, NOT this table.
const NAMED_KEY_SEQUENCES: Record<string, string> = {
  Enter: '\r',
  Backspace: '\x7f',
  Tab: '\t',
  Escape: '\x1b',
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

// Cursor keys whose encoding flips between CSI and SS3 under DECCKM. The final
// letter is shared between forms (CSI ? <letter> vs SS3 O <letter>).
const CURSOR_KEY_FINALS: Record<string, string> = {
  ArrowUp: 'A',
  ArrowDown: 'B',
  ArrowRight: 'C',
  ArrowLeft: 'D',
  Home: 'H',
  End: 'F'
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

/** Encode a cursor key under the active DECCKM mode: SS3 (ESC O <final>) in
 *  application-cursor mode, CSI (ESC [ <final>) otherwise. */
function encodeCursorKey(final: string, appCursor: boolean): string {
  return appCursor ? `\x1bO${final}` : `\x1b[${final}`
}

export function encodeKeyEventToBytes(
  event: KeyboardEvent,
  options: AtermKeyEncodingOptions = {}
): string | null {
  const { key } = event
  const appCursor = options.appCursor === true

  // Alt+Ctrl+<key> → ESC + control byte (meta-prefixed control), matching xterm.
  // Checked before the plain Ctrl branch (which excludes Alt) so the chord isn't
  // dropped as "unhandled".
  if (event.ctrlKey && event.altKey && !event.metaKey) {
    const control = encodeCtrlChord(key)
    return control === null ? null : `\x1b${control}`
  }

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

  // Cursor keys: mode-dependent SS3/CSI split (DECCKM).
  const cursorFinal = CURSOR_KEY_FINALS[key]
  if (cursorFinal) {
    return encodeCursorKey(cursorFinal, appCursor)
  }

  const named = NAMED_KEY_SEQUENCES[key]
  if (named) {
    return named
  }

  // Alt/Option+<printable> → ESC prefix (meta), the conventional terminal form.
  if (event.altKey && !event.ctrlKey && !event.metaKey && key.length === 1) {
    return `\x1b${key}`
  }

  // Plain printable single characters are NOT sent here: they flow through the
  // textarea 'input'/IME path so paste + composition + typing share one route
  // and nothing is double-sent. Return null so the caller leaves the event alone.
  return null
}
