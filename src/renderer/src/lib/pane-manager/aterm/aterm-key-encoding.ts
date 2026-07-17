// DOM keydown/keyup → engine-encoder bridge for the aterm helper textarea. The
// ENGINE
// owns byte production (legacy + xterm modifyOtherKeys + kitty CSI-u, driven by
// the live keyboard mode: DECCKM SS3, CSI 1;mod on arrows/nav, Ctrl-letter
// controls, Alt ESC-prefixing per 1036/1039). This module only extracts the
// engine's (key, mods, event_type, base_layout_key) tuple from the DOM event and
// applies the HOST platform gates the engine can't know about. Returns null when
// the event is not terminal input so the caller leaves it to the browser/app.
//
// Input model (mirrors xterm): keydown encodes ONLY non-text keys (Enter, Tab,
// arrows, editing/nav, F-keys, Ctrl/Alt chords). Plain printable characters
// return null and flow through the textarea 'input'/IME path instead, so they
// are never double-sent — EXCEPT under kitty REPORT_ALL_KEYS_AS_ESC, where the
// app negotiated escape reports for everything and printable presses go to the
// engine too (bytes ⇒ preventDefault ⇒ no input event follows, so the
// never-double-send property holds). Keyups encode every key (kitty
// REPORT_EVENT_TYPES releases); the engine emits nothing for them in legacy
// mode. See aterm-textarea-input.ts.

import {
  isGenuineWindowsCtrlAltChord,
  shouldRepairWindowsCtrlAltChords
} from '../terminal-windows-ctrl-alt-chord-classification'

/** Engine `Modifiers` bitfield (SHIFT=1 ALT=2 CTRL=4 SUPER=8). */
export const ATERM_KEY_MOD_SHIFT = 1
export const ATERM_KEY_MOD_ALT = 2
export const ATERM_KEY_MOD_CTRL = 4
export const ATERM_KEY_MOD_SUPER = 8

/** Engine `KeyEventType` (0=Press, 1=Repeat, 2=Release). The engine downgrades
 *  Repeat to Press and drops Release entirely unless the app negotiated kitty
 *  event-type reporting, so sending the true type is always safe. */
export const ATERM_KEY_EVENT_PRESS = 0
export const ATERM_KEY_EVENT_REPEAT = 1
export const ATERM_KEY_EVENT_RELEASE = 2

/** The engine encoder seam: `AtermTerminal.encode_key` in-process (live keyboard
 *  mode, exact) or the free `encode_key_with_mode` + snapshot mode bits on the
 *  worker path. Returns undefined when the event encodes to nothing. */
export type AtermEngineKeyEncoder = (
  key: string,
  mods: number,
  eventType: number,
  baseLayoutKey?: string | null
) => Uint8Array | undefined

/** `isMac` selects macOS Option semantics; `macOptionIsMeta` mirrors xterm's
 *  option of the same name — only when true does macOS Option act as Meta,
 *  otherwise the OS composes the glyph and we defer to the input event. On
 *  non-Mac these are ignored (Alt always reaches the engine as a chord).
 *  `getKeyboardModeBits` (read per event, so a mode flip applies immediately)
 *  lets the printable keydown gates stand down under kitty
 *  REPORT_ALL_KEYS_AS_ESC — that mode promises "text will not be sent", so
 *  printable presses must reach the engine encoder instead of the textarea
 *  input path. */
export type AtermKeyEncodingOptions = {
  isMac?: boolean
  macOptionIsMeta?: boolean
  getKeyboardModeBits?: () => number
}

/** KeyboardMode bit for kitty progressive flag 8, "report all keys as escape
 *  codes" (aterm_types::keyboard mode.rs: 1<<8). While active the app wants
 *  EVERY key — printables and standalone modifiers included — as an escape
 *  report, so host-side printable/modifier gating must stand down. */
export const ATERM_KEYBOARD_MODE_REPORT_ALL_KEYS_AS_ESC = 0x100

// Keyboard-mode bits (mirror aterm_types::keyboard::KeyboardMode) meaning the
// app negotiated an enhanced key protocol — kitty progressive enhancement
// (disambiguate/event-types/alternate/all-keys/associated-text) or xterm
// modifyOtherKeys/formatOtherKeys — and therefore wants the REAL encoded chord.
const APP_KEY_PROTOCOL_MASK = 0x1 | 0x2 | 0x10 | 0x20 | 0x40 | 0x80 | 0x100 | 0x200

/** True when the pane's app negotiated kitty / modifyOtherKeys, so host
 *  readline-compat chord rewrites must stand down and let the engine encode. */
export function atermAppKeyProtocolNegotiated(keyboardModeBits: number): boolean {
  return (keyboardModeBits & APP_KEY_PROTOCOL_MASK) !== 0
}

// The engine returns UTF-8 bytes; the PTY input sinks speak strings.
const KEY_BYTES_DECODER = new TextDecoder()

/** The engine `Modifiers` bitfield for a DOM keyboard event. */
export function atermModsFromEvent(event: KeyboardEvent): number {
  return (
    (event.shiftKey ? ATERM_KEY_MOD_SHIFT : 0) |
    (event.altKey ? ATERM_KEY_MOD_ALT : 0) |
    (event.ctrlKey ? ATERM_KEY_MOD_CTRL : 0) |
    (event.metaKey ? ATERM_KEY_MOD_SUPER : 0)
  )
}

/** US-QWERTY character of the physical key (KeyA→'a') when it differs from the
 *  logical key — kitty REPORT_ALTERNATE_KEYS consumers match hotkeys by it on
 *  non-Latin layouts. Undefined when unknown or redundant. */
export function atermBaseLayoutKey(event: KeyboardEvent): string | undefined {
  const code = event.code
  if (!code || code.length !== 4 || !code.startsWith('Key')) {
    return undefined
  }
  const base = code.charAt(3).toLowerCase()
  return base === event.key.toLowerCase() ? undefined : base
}

// One grapheme = printable text (composed glyphs like 'å' included); longer
// strings are named DOM keys ('Enter', 'ArrowUp', 'Dead', …).
function isPrintableKey(key: string): boolean {
  return Array.from(key).length === 1
}

export function encodeKeyEventToBytes(
  event: KeyboardEvent,
  encodeWithEngine: AtermEngineKeyEncoder,
  options: AtermKeyEncodingOptions = {}
): string | null {
  const isMac = options.isMac === true
  const macOptionIsMeta = options.macOptionIsMeta === true
  const isRelease = event.type === 'keyup'
  const { key } = event

  // Cmd/Super chords are app shortcuts (menu/copy/tab nav), NOT terminal input:
  // never encode them (press OR release). The caller checks its copy-chord
  // first, so a handled Cmd+C never reaches here; everything else returns null
  // so the app owns it.
  if (event.metaKey) {
    return null
  }

  // Under kitty REPORT_ALL_KEYS_AS_ESC the app negotiated "text will not be
  // sent": every printable press/repeat must be ENGINE-encoded (ESC report),
  // not routed through the textarea input path as raw text. Skipping the
  // gates below cannot double-send: when the engine returns bytes the caller
  // preventDefaults the keydown, so no input event follows; and when it
  // returns nothing we fall back to null (text path) so keys never go dead.
  // The metaKey firewall above still wins (Cmd chords stay app-domain — a
  // documented host divergence), and the caller checks IME composition first.
  const reportAllKeysActive =
    options.getKeyboardModeBits !== undefined &&
    (options.getKeyboardModeBits() & ATERM_KEYBOARD_MODE_REPORT_ALL_KEYS_AS_ESC) !== 0

  // The printable gates below exist to avoid double-sending with the textarea
  // 'input' path — keyups fire no input event, so releases skip them and let
  // the engine decide (it drops releases outside kitty event-type reporting).
  if (!isRelease && !reportAllKeysActive && isPrintableKey(key)) {
    // On macOS Option only acts as Meta when macOptionIsMeta is on; with it OFF
    // (the default) the OS composes the glyph ('å' for Option+a) and the input
    // event delivers it — keydown must NOT also encode the chord.
    const altActsAsMeta = event.altKey && (!isMac || macOptionIsMeta)
    // Plain/shifted printables flow through the textarea 'input'/IME path (one
    // route for typing, paste, and composition — never double-sent); only a
    // Ctrl or Alt-as-Meta chord makes a printable keydown terminal input.
    if (!event.ctrlKey && !altActsAsMeta) {
      return null
    }
    // Ctrl+Alt on a printable is AltGr composition on Windows/Linux layouts
    // ('@' on German). Only letter / C0-symbol chords are terminal input; other
    // composed glyphs stay on the input path — EXCEPT when Chromium's AltGraph
    // rewrite proves this Windows Ctrl+Alt press cannot compose (#8810): a
    // genuine chord must reach the engine encoder instead of going dead.
    if (event.ctrlKey && event.altKey && !/^[a-zA-Z[\]\\_ ]$/.test(key)) {
      const chordCannotCompose =
        typeof navigator !== 'undefined' &&
        shouldRepairWindowsCtrlAltChords(navigator.userAgent) &&
        isGenuineWindowsCtrlAltChord(event)
      if (!chordCannotCompose) {
        return null
      }
    }
  }

  // True event type always: the engine downgrades Repeat→Press and drops
  // Release when kitty event-type reporting is off (legacy semantics).
  const eventType = isRelease
    ? ATERM_KEY_EVENT_RELEASE
    : event.repeat
      ? ATERM_KEY_EVENT_REPEAT
      : ATERM_KEY_EVENT_PRESS
  const bytes = encodeWithEngine(
    key,
    atermModsFromEvent(event),
    eventType,
    atermBaseLayoutKey(event)
  )
  // Undefined/empty = no terminal encoding (modifier-only, IME, unmapped named
  // keys) — leave the event to the browser/app.
  if (!bytes || bytes.length === 0) {
    return null
  }
  return KEY_BYTES_DECODER.decode(bytes)
}
