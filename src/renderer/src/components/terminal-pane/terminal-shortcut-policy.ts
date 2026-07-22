import {
  keybindingChordHasNoNonShiftModifiers,
  keybindingMatchesAction,
  type KeybindingActionId,
  type KeybindingOverrides
} from '../../../../shared/keybindings'
import {
  matchCustomKeybinding,
  type ResolvedCustomKeybinding
} from '../../../../shared/custom-keybindings'
import type { WindowsShiftEnterEncoding } from './terminal-windows-shift-enter'

export type TerminalShortcutEvent = {
  key: string
  code?: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
  repeat?: boolean
  // Why: threaded from the DOM event so custom entries can hard-gate on open IME compositions.
  isComposing?: boolean
}

export type MacOptionAsAlt = 'true' | 'false' | 'left' | 'right'

// Why: macOS composition rewrites event.key for punctuation, so map event.code to the unmodified char for Esc+ sequences.
const PUNCTUATION_CODE_MAP: Record<string, string> = {
  Period: '.',
  Comma: ',',
  Slash: '/',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  BracketLeft: '[',
  BracketRight: ']',
  Minus: '-',
  Equal: '=',
  Backquote: '`'
}

export type TerminalShortcutAction =
  | { type: 'copySelection' }
  | { type: 'toggleSearch' }
  | { type: 'clearActivePane' }
  | { type: 'focusPane'; direction: 'next' | 'previous' }
  | { type: 'equalizePaneSizes' }
  | { type: 'toggleExpandActivePane' }
  | { type: 'setTitle' }
  | { type: 'clearPaneTitle' }
  | { type: 'closeActivePane' }
  | { type: 'splitActivePane'; direction: 'vertical' | 'horizontal' }
  | { type: 'scrollViewport'; position: 'top' | 'bottom' }
  // suppressTextInsertion: set for bare/Shift-only custom chords — the handler must also
  // swallow the key's companion keypress/beforeinput so the character is never typed.
  | { type: 'sendInput'; data: string; suppressTextInsertion?: boolean }
  // Run a user-configured terminal Quick Command (custom keybinding action).
  | { type: 'runQuickCommand'; quickCommandId: string }
  // Fully swallow the keystroke (repeat of a once-per-press custom chord).
  | { type: 'consumeKey' }
  // Encode `key`+`mods` through the ACTIVE pane's engine (live keyboard mode)
  // and send the result — for chords the browser/OS would otherwise mangle
  // (Cmd chords hard-nulled by the encoder's metaKey firewall, macOS Option
  // composition replacing event.key). Only emitted when the pane negotiated an
  // enhanced key protocol: the ENGINE picks the negotiated dialect (kitty CSI-u
  // vs xterm modifyOtherKeys), which this policy must not hard-code. `fallback`
  // is the legacy bytes the ungated rewrite would have sent — used when the
  // engine returns nothing, so the chord never goes dead.
  | {
      type: 'encodeKey'
      key: string
      mods: { alt?: boolean; super?: boolean }
      fallback: string
    }
  | { type: 'switchInputSource' }
  | { type: 'toggleComposeBox' }

// Why: the repeat-precedence guard for custom entries must mirror the !repeat ladder in
// resolveTerminalShortcutAction — keep this list in sync with the keybindingMatchesAction calls there.
const REPEAT_GATED_TERMINAL_ACTION_IDS: readonly KeybindingActionId[] = [
  'terminal.copySelection',
  'terminal.search',
  'terminal.clear',
  'terminal.focusPreviousPane',
  'terminal.focusNextPane',
  'terminal.equalizePaneSizes',
  'terminal.expandPane',
  'terminal.setTitle',
  'terminal.clearPaneTitle',
  'terminal.closePane',
  'terminal.splitRight',
  'terminal.splitDown',
  'terminal.composeBox'
]

/** Un-shifted ASCII character for a physical key code (letters, digits, punctuation map), or undefined. */
function resolveUnshiftedCharacterForCode(code: string | undefined): string | undefined {
  if (!code) {
    return undefined
  }
  if (code.startsWith('Key') && code.length === 4) {
    return code.charAt(3).toLowerCase()
  }
  if (code.startsWith('Digit') && code.length === 6) {
    return code.charAt(5)
  }
  return PUNCTUATION_CODE_MAP[code]
}

/**
 * Resolves terminal keyboard events before the engine receives them, centralizing
 * Orca shortcuts and terminal byte fallbacks in one platform-aware policy.
 */
export function resolveTerminalShortcutAction(
  event: TerminalShortcutEvent,
  isMac: boolean,
  macOptionAsAlt: MacOptionAsAlt = 'false',
  optionKeyLocation: number = 0,
  isWindows: boolean = false,
  keybindings?: KeybindingOverrides,
  // Why: lazy so execution-host lookup (local native Windows ConPTY) runs only on Ctrl+Arrow, not every keystroke.
  isLocalWindowsConptyPane?: () => boolean,
  // Why: stands the legacy readline-compat rewrites down once the pane's app negotiates an enhanced key protocol.
  // The fork feeds the aterm engine's negotiated signal (atermAppKeyProtocolNegotiated(keyboardModeBits())) — which
  // includes modifyOtherKeys, not just kitty CSI > u — so modifyOtherKeys panes gate too.
  isKittyKeyboardActivePane?: () => boolean,
  // Why: the physical-code table above is US QWERTY; resolve via Chromium's KeyboardLayoutMap for Dvorak/Colemak/AZERTY layouts.
  layoutBaseCharacterForCode?: (code: string) => string | undefined,
  // Why: lazy so agent-state lookup for the pane's Windows encoding runs only on Shift+Enter, not every keystroke.
  getWindowsShiftEnterEncoding?: () => WindowsShiftEnterEncoding,
  // Why: keybindings follow the client OS, but byte protocols follow the PTY host — they differ for macOS clients on Windows runtimes.
  isWindowsTerminalHost: () => boolean = () => isWindows,
  customKeybindings?: readonly ResolvedCustomKeybinding[]
): TerminalShortcutAction | null {
  const platform: NodeJS.Platform = isMac ? 'darwin' : isWindows ? 'win32' : 'linux'

  // Why: capture this chord even on repeat without blocking the OS default input-source switch.
  if (keybindingMatchesAction('terminal.switchInputSource', event, platform, keybindings)) {
    return { type: 'switchInputSource' }
  }

  if (!event.repeat) {
    if (keybindingMatchesAction('terminal.copySelection', event, platform, keybindings)) {
      return { type: 'copySelection' }
    }

    if (keybindingMatchesAction('terminal.search', event, platform, keybindings)) {
      return { type: 'toggleSearch' }
    }

    if (keybindingMatchesAction('terminal.clear', event, platform, keybindings)) {
      return { type: 'clearActivePane' }
    }

    if (keybindingMatchesAction('terminal.focusPreviousPane', event, platform, keybindings)) {
      return { type: 'focusPane', direction: 'previous' }
    }

    if (keybindingMatchesAction('terminal.focusNextPane', event, platform, keybindings)) {
      return { type: 'focusPane', direction: 'next' }
    }

    if (keybindingMatchesAction('terminal.equalizePaneSizes', event, platform, keybindings)) {
      return { type: 'equalizePaneSizes' }
    }

    if (keybindingMatchesAction('terminal.expandPane', event, platform, keybindings)) {
      return { type: 'toggleExpandActivePane' }
    }

    if (keybindingMatchesAction('terminal.setTitle', event, platform, keybindings)) {
      return { type: 'setTitle' }
    }

    if (keybindingMatchesAction('terminal.clearPaneTitle', event, platform, keybindings)) {
      return { type: 'clearPaneTitle' }
    }

    if (keybindingMatchesAction('terminal.closePane', event, platform, keybindings)) {
      return { type: 'closeActivePane' }
    }

    if (keybindingMatchesAction('terminal.splitRight', event, platform, keybindings)) {
      return { type: 'splitActivePane', direction: 'vertical' }
    }

    if (keybindingMatchesAction('terminal.splitDown', event, platform, keybindings)) {
      return { type: 'splitActivePane', direction: 'horizontal' }
    }

    if (keybindingMatchesAction('terminal.composeBox', event, platform, keybindings)) {
      return { type: 'toggleComposeBox' }
    }
  }

  // Custom user entries sit between the configurable ladder above (built-ins win a shared
  // chord) and the hardcoded byte rewrites below (a user remap of e.g. Shift+Enter must win).
  // Hard IME gate: never match mid-composition — candidate-window keystrokes stay untouched.
  if (event.isComposing !== true && event.key !== 'Process' && customKeybindings?.length) {
    const custom = matchCustomKeybinding(customKeybindings, event, platform)
    // Why: repeats skip the !repeat ladder above, so a held built-in chord would otherwise
    // fall through to a same-chord custom entry; write-time conflict blocking is only defense #1.
    const shadowedByBuiltIn =
      custom !== null &&
      REPEAT_GATED_TERMINAL_ACTION_IDS.some((actionId) =>
        keybindingMatchesAction(actionId, event, platform, keybindings)
      )
    if (custom && !shadowedByBuiltIn) {
      if (custom.entry.action.type === 'runQuickCommand') {
        // Why: command-like customs are once-per-press; swallowing repeats keeps a held chord
        // from falling through to the byte rewrites below or reaching the engine encoder.
        return event.repeat
          ? { type: 'consumeKey' }
          : { type: 'runQuickCommand', quickCommandId: custom.entry.action.quickCommandId }
      }
      if (custom.entry.decodedText !== undefined) {
        // sendText fires on key repeat — it substitutes for typing, so a held remapped key auto-repeats.
        return {
          type: 'sendInput',
          data: custom.entry.decodedText,
          suppressTextInsertion: keybindingChordHasNoNonShiftModifiers(custom.binding)
        }
      }
    }
  }

  // Once the active pane's app has negotiated an enhanced key protocol the
  // engine encoder emits the real chord, so the readline-compat rewrites below
  // stand down. Resolved lazily and memoized so the lookup stays off the hot
  // path for keystrokes that never consult it (gates put it last, after the
  // cheap modifier/key checks short-circuit).
  let kittyKeyboardActiveMemo: boolean | undefined
  const kittyKeyboardActive = (): boolean => {
    kittyKeyboardActiveMemo ??= isKittyKeyboardActivePane?.() === true
    return kittyKeyboardActiveMemo
  }

  if (
    !event.metaKey &&
    !event.ctrlKey &&
    !event.altKey &&
    event.shiftKey &&
    event.key === 'Enter'
  ) {
    // Why: CSI-u (\x1b[13;2u) is application input, not universal — send it only on active KKP negotiation OR a trusted
    // Windows agent (e.g. Droid) that wants CSI-u without renderer-visible KKP (host ≠ client, so the aterm engine can't
    // know); otherwise the universal Alt+Enter byte (\x1b\r). Short-circuit on windowsHost keeps the lookup off non-Windows keystrokes.
    const windowsHost = isWindowsTerminalHost()
    const hasTrustedWindowsCsiU = windowsHost && getWindowsShiftEnterEncoding?.() === 'csi-u'
    const canSendCsiU = hasTrustedWindowsCsiU || kittyKeyboardActive()
    return { type: 'sendInput', data: canSendCsiU ? '\x1b[13;2u' : '\x1b\r' }
  }

  if (
    event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    !event.shiftKey &&
    event.key === 'Enter' &&
    !kittyKeyboardActive()
  ) {
    // Why: legacy encoding collapses Ctrl+Enter to a bare CR, so forward kitty CSI-u (modifier 5 = Ctrl) so the chord
    // reaches TUIs; with kitty/modifyOtherKeys negotiated the engine encoder already emits the app's chosen form, so
    // this stands down. No Windows fallback yet (#2418).
    return { type: 'sendInput', data: '\x1b[13;5u' }
  }

  if (
    event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    !event.shiftKey &&
    event.key === 'Backspace' &&
    !kittyKeyboardActive()
  ) {
    // Readline-compat delete-word; kitty-gated because the engine encodes the
    // real chord (\x1b[127;5u) for negotiated apps.
    return { type: 'sendInput', data: '\x17' }
  }

  if (isMac && event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey) {
    // Why encodeKey (not a silent stand-down) under a negotiated protocol: the
    // aterm keydown encoder hard-nulls ALL metaKey events (Cmd chords are app
    // domain), so standing down would make these chords go DEAD on negotiated
    // apps. Route them back through the engine with SUPER so the app receives
    // the real modified key in its negotiated dialect.
    if (event.key === 'Backspace') {
      return kittyKeyboardActive()
        ? { type: 'encodeKey', key: 'Backspace', mods: { super: true }, fallback: '\x15' }
        : { type: 'sendInput', data: '\x15' }
    }
    if (event.key === 'Delete') {
      return kittyKeyboardActive()
        ? { type: 'encodeKey', key: 'Delete', mods: { super: true }, fallback: '\x0b' }
        : { type: 'sendInput', data: '\x0b' }
    }
    // Why: the engine has no Cmd+Arrow mapping; translate Cmd+←/→ to readline Ctrl+A/Ctrl+E for line start/end (iTerm2/Ghostty).
    if (event.key === 'ArrowLeft') {
      return kittyKeyboardActive()
        ? { type: 'encodeKey', key: 'ArrowLeft', mods: { super: true }, fallback: '\x01' }
        : { type: 'sendInput', data: '\x01' }
    }
    if (event.key === 'ArrowRight') {
      return kittyKeyboardActive()
        ? { type: 'encodeKey', key: 'ArrowRight', mods: { super: true }, fallback: '\x05' }
        : { type: 'sendInput', data: '\x05' }
    }
    // Why: macOS users expect Cmd+↑/↓ to scroll scrollback, not write escape bytes to the shell.
    // Host action — legitimate even for kitty apps, so deliberately NOT protocol-gated.
    if (event.key === 'ArrowUp') {
      return { type: 'scrollViewport', position: 'top' }
    }
    if (event.key === 'ArrowDown') {
      return { type: 'scrollViewport', position: 'bottom' }
    }
  }

  if (
    !event.metaKey &&
    !event.ctrlKey &&
    event.altKey &&
    !event.shiftKey &&
    event.key === 'Backspace' &&
    !kittyKeyboardActive()
  ) {
    // Readline-compat delete-word (backward); kitty-gated because the engine
    // encodes the real chord (\x1b[127;3u) for negotiated apps.
    return { type: 'sendInput', data: '\x1b\x7f' }
  }

  if (
    !event.metaKey &&
    !event.ctrlKey &&
    event.altKey &&
    !event.shiftKey &&
    (event.key === 'ArrowLeft' || event.key === 'ArrowRight') &&
    !kittyKeyboardActive()
  ) {
    // Why: readline doesn't bind the legacy \e[1;3D/C for alt+←/→, so translate to \eb/\ef for word-nav (iTerm2 "Esc+"
    // behavior; platform-agnostic since both produce altKey). Protocol-gated: a negotiated app wants the real modified-arrow report.
    return { type: 'sendInput', data: event.key === 'ArrowLeft' ? '\x1bb' : '\x1bf' }
  }

  if (
    !isMac &&
    !event.metaKey &&
    event.ctrlKey &&
    !event.altKey &&
    !event.shiftKey &&
    (event.key === 'ArrowLeft' || event.key === 'ArrowRight') &&
    !kittyKeyboardActive()
  ) {
    // Why: local Windows ConPTY (PSReadLine) binds Ctrl+←/→ itself and treats \eb/\ef as Escape→RevertLine plus a stray
    // b/f — stand down and let the engine encoder emit its native \e[1;5D/C. Remote/WSL panes run readline and still
    // need the translation, so this gates on a genuine local native ConPTY, not merely a Windows client.
    if (isLocalWindowsConptyPane?.()) {
      return null
    }
    // Why: readline ignores the legacy \e[1;5D/C, so translate Ctrl+←/→ to \eb/\ef for word-nav; protocol-gated like the
    // Alt+Arrow rule. !isMac since macOS reserves Ctrl+Arrow for Mission Control / Spaces.
    return { type: 'sendInput', data: event.key === 'ArrowLeft' ? '\x1bb' : '\x1bf' }
  }

  // Why: with macOptionIsMeta disabled (to let non-US keyboard layouts compose
  // characters like @ and €), the engine no longer translates Option+letter into
  // Esc+letter automatically. We match on event.code (physical key) rather than
  // event.key because macOS composition replaces event.key with the composed
  // character (e.g. Option+B reports key='∫', not key='b').
  //
  // The handling depends on the macOptionAsAlt setting (mirrors Ghostty):
  // - 'true':  Option is handled as Meta natively; nothing to do here.
  // - 'false': compensate the three most critical readline shortcuts (B/F/D).
  // - 'left'/'right': the designated Option key acts as full Meta (emit Esc+
  //   for any single letter); the other key composes, with B/F/D compensated.
  if (isMac && !event.metaKey && !event.ctrlKey && event.altKey && !event.shiftKey) {
    // Why: event.location on a character key reports that key's position (always
    // 0 for standard keys), NOT which modifier is held. The caller must track
    // the Option key's own keydown location and pass it as optionKeyLocation.
    const isLeftOption = optionKeyLocation === 1
    const isRightOption = optionKeyLocation === 2

    const shouldActAsMeta =
      (macOptionAsAlt === 'left' && isLeftOption) || (macOptionAsAlt === 'right' && isRightOption)

    // Why encodeKey under a negotiated protocol: the app wants the real Alt+key
    // report in ITS dialect, not a hard-coded Esc+letter (or hard-coded kitty
    // CSI-u — kittyKeyboardActive is also true for modifyOtherKeys-only panes,
    // so the ENGINE must pick the form from live mode bits). `baseKey` is the
    // layout-true unshifted char for the negotiated report (event.code alone
    // reports the wrong key on Dvorak/Colemak/AZERTY); the legacy Esc+ fallback
    // keeps the physical-code char that readline word-nav expects.
    const encodeOptionChord = (
      physicalChar: string,
      baseKey: string = physicalChar
    ): TerminalShortcutAction => {
      return kittyKeyboardActive()
        ? { type: 'encodeKey', key: baseKey, mods: { alt: true }, fallback: `\x1b${physicalChar}` }
        : { type: 'sendInput', data: `\x1b${physicalChar}` }
    }

    if (shouldActAsMeta) {
      // Emit Esc+key (Option+B → \x1bb) for letters, digits, and mapped
      // punctuation; the negotiated base key resolves through the active layout.
      const physicalChar = resolveUnshiftedCharacterForCode(event.code)
      if (physicalChar) {
        const layoutChar =
          (event.code ? layoutBaseCharacterForCode?.(event.code) : undefined) ?? physicalChar
        return encodeOptionChord(physicalChar, layoutChar)
      }
    }

    // In 'false', 'left', or 'right' mode, the compose-side Option key still
    // needs the three most critical readline shortcuts patched (Alt+B/F/D).
    if (macOptionAsAlt !== 'true' && !shouldActAsMeta) {
      if (event.code === 'KeyB') {
        return encodeOptionChord('b')
      }
      if (event.code === 'KeyF') {
        return encodeOptionChord('f')
      }
      if (event.code === 'KeyD') {
        return encodeOptionChord('d')
      }
    }
  }

  return null
}
