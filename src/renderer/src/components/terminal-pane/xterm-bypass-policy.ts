import { keybindingMatchesInput } from '../../../../shared/keybindings'
import { ATERM_KEYBOARD_MODE_REPORT_ALL_KEYS_AS_ESC } from '@/lib/pane-manager/aterm/aterm-key-encoding'

// Why: when a CLI activates kitty progressive enhancement (CSI > N u), xterm's
// KittyKeyboard encoder turns every modifier chord — including plain Cmd+C —
// into a CSI-u sequence with `cancel: true`, which calls preventDefault() on
// the keydown. That preventDefault suppresses Chromium's native `copy` event,
// so xterm's own `copy` listener on its container never fires and the
// selection is never written to the clipboard.
//
// Fix: intercept in `attachCustomKeyEventHandler` and return `false` for chords
// that should bubble to the browser / host (clipboard, native menu). Returning
// `false` makes xterm bail *before* the kitty encoder runs, so the browser's
// copy pipeline and the OS-level keybinding both fire normally.

export type XtermBypassEvent = {
  type: string
  key: string
  code?: string
  keyCode?: number
  isComposing?: boolean
  defaultPrevented?: boolean
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
}

export type XtermBypassOptions = {
  isMac: boolean
  /** True when the terminal has a current text selection — Ctrl+C on
   *  Windows/Linux should only bubble to clipboard when something is selected,
   *  otherwise it must reach the shell as SIGINT. */
  hasSelection: boolean
}

export type XtermImeKeyboardOptions = {
  compositionActive: boolean
  // Required so no caller silently falls back to non-mac 229 suppression,
  // which re-swallows the first key after a macOS IME input-source switch.
  isMac: boolean
}

export const TERMINAL_INTERRUPT_INPUT = '\x03'
/** The kitty CSI-u form of a plain Ctrl+C press ('c' = 99, modifiers 5 = Ctrl):
 *  what a kitty-negotiated aterm pane sends instead of raw ETX now that the
 *  host interrupt claim stands down for negotiated apps (Claude Code under
 *  CSI > 1 u receives its interrupt this way). Interrupt-intent detectors must
 *  accept BOTH forms. */
export const TERMINAL_INTERRUPT_INPUT_KITTY = '\x1b[99;5u'
/** The kitty CSI-u form of a plain Escape press (key 27) under disambiguate —
 *  the escape-intent twin of TERMINAL_INTERRUPT_INPUT_KITTY. */
export const TERMINAL_ESCAPE_INPUT_KITTY = '\x1b[27u'
const TERMINAL_MODIFIER_KEYS = new Set(['Alt', 'AltGraph', 'Control', 'Meta', 'Shift'])
const TERMINAL_IME_OWNED_KEYS = new Set([
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'Backspace',
  'Delete',
  'End',
  'Enter',
  'Escape',
  'Home',
  'PageDown',
  'PageUp'
])

function isSingleNonAsciiPrintableText(key: string): boolean {
  const chars = Array.from(key)
  if (chars.length !== 1) {
    return false
  }
  const codePoint = chars[0].codePointAt(0)
  return codePoint !== undefined && codePoint >= 0x80
}

function isXtermHandledKeyEvent(type: string): boolean {
  return type === 'keydown' || type === 'keyup'
}

export function shouldSuppressTerminalImeKeyboardEvent(
  event: XtermBypassEvent,
  options: XtermImeKeyboardOptions
): boolean {
  if (!isXtermHandledKeyEvent(event.type)) {
    return false
  }
  const { compositionActive, isMac } = options
  // Why: IMEs own Process-key / composing keystrokes — letting xterm translate
  // them corrupts committed CJK text. Bare macOS keydown 229 is exempt: it must
  // reach xterm's CompositionHelper or the first key after an input-source
  // switch is swallowed.
  return (
    event.isComposing === true ||
    (event.keyCode === 229 && (event.type !== 'keydown' || compositionActive || !isMac)) ||
    (compositionActive && TERMINAL_IME_OWNED_KEYS.has(event.key))
  )
}

function isTerminalInterruptCKey(event: XtermBypassEvent): boolean {
  const normalizedKey = event.key.toLowerCase()
  const logicalKeyAvailable = normalizedKey !== '' && normalizedKey !== 'unidentified'
  return logicalKeyAvailable ? normalizedKey === 'c' : event.code === 'KeyC' || event.keyCode === 67
}

function isPlainCtrlC(event: XtermBypassEvent): boolean {
  return (
    isTerminalInterruptCKey(event) &&
    event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    !event.shiftKey
  )
}

function matchesClipboardBinding(
  binding: string,
  event: XtermBypassEvent,
  platform: NodeJS.Platform
): boolean {
  return keybindingMatchesInput(binding, event, platform)
}

/**
 * Decide whether plain Ctrl+C should bypass xterm's kitty CSI-u encoder and
 * be sent as ETX through Terminal.input() instead.
 */
export function shouldHandleTerminalInterruptKeyboardEvent(
  event: XtermBypassEvent,
  options: XtermBypassOptions
): boolean {
  if (!isXtermHandledKeyEvent(event.type) || !isPlainCtrlC(event)) {
    return false
  }

  if (options.isMac) {
    return true
  }

  return !options.hasSelection
}

export type TerminalInterruptClaimOptions = XtermBypassOptions & {
  /** True when the pane's app negotiated kitty / modifyOtherKeys
   *  (atermAppKeyProtocolNegotiated over the live engine mode bits). */
  appKeyProtocolNegotiated: boolean
}

/**
 * The full interrupt-claim decision: claim plain Ctrl+C (send raw ETX, own the
 * paired keyup) ONLY for panes whose app has NOT negotiated an enhanced key
 * protocol. A negotiated pane stands down entirely — the engine encoder emits
 * the app's negotiated interrupt form (ESC[99;5u under kitty) from live mode
 * bits, its release gating owns the keyup, and the app keeps its flags (the
 * old unconditional claim also reset kitty state, desyncing apps that survive
 * Ctrl+C, like Claude Code).
 */
export function shouldClaimTerminalInterruptKeyboardEvent(
  event: XtermBypassEvent,
  options: TerminalInterruptClaimOptions
): boolean {
  if (options.appKeyProtocolNegotiated) {
    return false
  }
  return shouldHandleTerminalInterruptKeyboardEvent(event, options)
}

export function shouldSuppressTerminalInterruptKeyup(event: XtermBypassEvent): boolean {
  return (
    event.type === 'keyup' &&
    isTerminalInterruptCKey(event) &&
    !event.metaKey &&
    !event.altKey &&
    !event.shiftKey
  )
}

export type XtermModifierSuppressionOptions = {
  /** The pane's live engine KeyboardMode bitfield (0 when unavailable — e.g.
   *  no aterm controller yet — which keeps the legacy suppression). */
  keyboardModeBits: number
}

/**
 * Suppress standalone modifier key events (Shift/Control/Alt/Meta/AltGraph)
 * before the engine encoder — UNLESS the app negotiated kitty
 * REPORT_ALL_KEYS_AS_ESC, which explicitly asks for modifier press/release
 * reports. The engine maps "Shift"→ShiftLeft (Left-canonical) and gates the
 * report on that mode bit itself, so outside report-all this suppression is
 * belt-and-braces; under report-all it must stand down or those apps never
 * see their modifier events.
 */
export function shouldSuppressTerminalModifierKeyboardEvent(
  event: XtermBypassEvent,
  options?: XtermModifierSuppressionOptions
): boolean {
  if (!isXtermHandledKeyEvent(event.type) || !TERMINAL_MODIFIER_KEYS.has(event.key)) {
    return false
  }
  const keyboardModeBits = options?.keyboardModeBits ?? 0
  return (keyboardModeBits & ATERM_KEYBOARD_MODE_REPORT_ALL_KEYS_AS_ESC) === 0
}

/**
 * Decide whether a chord should bypass xterm's key handlers so the native
 * browser pipeline (Chromium `copy` event, Electron menu accelerators) or
 * layout-aware text event can handle it instead of the kitty CSI-u encoder.
 */
export function shouldBypassXtermKeyboardEvent(
  event: XtermBypassEvent,
  options: XtermBypassOptions
): boolean {
  if (!isXtermHandledKeyEvent(event.type)) {
    return false
  }

  const { isMac, hasSelection } = options
  const platformModifierHeld = isMac
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey

  if (event.defaultPrevented && platformModifierHeld) {
    // Why: window-level Orca shortcuts may have already handled the chord but
    // not stopped propagation. Do not let xterm also send that shortcut to
    // the shell.
    return true
  }

  if (
    event.shiftKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    isSingleNonAsciiPrintableText(event.key)
  ) {
    // Why: xterm's kitty encoder derives shifted key codes from physical
    // `code` (KeyA -> Latin "a"). Bypass keydown so Chromium emits layout text
    // via keypress, and bypass keyup so xterm doesn't leak the release CSI-u.
    return true
  }

  if (isMac) {
    // Why: window-level handlers already consume other Cmd chords before xterm
    // sees them in Electron. Web clients still need paste to bubble to
    // Chromium's native paste event instead of xterm's Kitty encoder.
    return (
      matchesClipboardBinding('Mod+C', event, 'darwin') ||
      matchesClipboardBinding('Mod+V', event, 'darwin')
    )
  }

  // Windows/Linux: standard clipboard bindings bubble; Ctrl+C only bubbles
  // with a selection (otherwise it's SIGINT and must reach the shell).
  if (matchesClipboardBinding('Ctrl+Shift+C', event, 'linux')) {
    return true
  }
  if (matchesClipboardBinding('Ctrl+C', event, 'linux') && hasSelection) {
    return true
  }
  if (
    matchesClipboardBinding('Ctrl+V', event, 'linux') ||
    matchesClipboardBinding('Ctrl+Shift+V', event, 'linux')
  ) {
    return true
  }
  if (matchesClipboardBinding('Shift+Insert', event, 'linux')) {
    return true
  }

  return false
}
