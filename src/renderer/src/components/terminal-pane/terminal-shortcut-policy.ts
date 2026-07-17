import { keybindingMatchesAction, type KeybindingOverrides } from '../../../../shared/keybindings'
import type { WindowsShiftEnterEncoding } from './terminal-windows-shift-enter'

export type TerminalShortcutEvent = {
  key: string
  code?: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
  repeat?: boolean
}

export type MacOptionAsAlt = 'true' | 'false' | 'left' | 'right'

// Why: macOS composition replaces event.key for punctuation, so we map
// event.code to the unmodified character for Esc+ sequences.
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
  | { type: 'sendInput'; data: string }
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

/** The un-shifted ASCII character for a physical key code (letters, digits,
 *  and the punctuation map above), or undefined for unmapped codes. */
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
 * Resolves terminal keyboard events before the engine receives them.
 * Keeps configurable Orca shortcuts and terminal byte fallbacks in one
 * platform-aware policy so renderer handlers do not duplicate key checks.
 */
export function resolveTerminalShortcutAction(
  event: TerminalShortcutEvent,
  isMac: boolean,
  macOptionAsAlt: MacOptionAsAlt = 'false',
  optionKeyLocation: number = 0,
  isWindows: boolean = false,
  keybindings?: KeybindingOverrides,
  // Why: lazily reports whether the active pane is a local native Windows
  // ConPTY (PowerShell/cmd via PSReadLine). Only consulted for the Ctrl+Arrow
  // word-nav rule below, so the execution-host lookup it performs stays off the
  // hot path for every other keystroke.
  isLocalWindowsConptyPane?: () => boolean,
  // Why: the readline-compat sendInput rewrites below exist for LEGACY-mode
  // apps; once the active pane's app negotiates an enhanced key protocol (kitty
  // OR xterm modifyOtherKeys) it wants the real encoded chord, so the caller
  // reports the pane's state here to stand the rewrites down and let the engine
  // encoder speak. The fork feeds the aterm engine's negotiated signal
  // (atermAppKeyProtocolNegotiated(keyboardModeBits())) — which includes
  // modifyOtherKeys, not just kitty CSI > u — so modifyOtherKeys panes gate too.
  isKittyKeyboardActivePane?: () => boolean,
  // Why: kitty/Option chords carry the key's unshifted codepoint in the active
  // layout; the physical-code table above is US QWERTY and reports the wrong
  // key on Dvorak/Colemak/AZERTY-class layouts. This resolves through
  // Chromium's KeyboardLayoutMap when it is available.
  layoutBaseCharacterForCode?: (code: string) => string | undefined,
  // Why: lazily resolves the active pane's Windows encoding. Only consulted for
  // Shift+Enter so agent-state lookup stays off every other keystroke.
  getWindowsShiftEnterEncoding?: () => WindowsShiftEnterEncoding,
  // Why: keybindings follow the client OS, but terminal byte protocols follow
  // the PTY host. They differ for macOS clients attached to Windows runtimes.
  isWindowsTerminalHost: () => boolean = () => isWindows
): TerminalShortcutAction | null {
  const platform: NodeJS.Platform = isMac ? 'darwin' : isWindows ? 'win32' : 'linux'

  // Why: native-only chords must be captured even on repeat without blocking
  // the OS default that performs the input-source switch.
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
    // Resolve Shift+Enter to explicit bytes so a composer newline works across
    // hosts. CSI-u (\x1b[13;2u) only when it is safe application input — an
    // active KKP negotiation OR a trusted Windows agent (e.g. Droid) that wants
    // CSI-u without renderer-visible KKP (host ≠ client, so the aterm engine
    // can't know); otherwise the universal Alt+Enter byte (\x1b\r). Short-circuit
    // on windowsHost keeps the encoding lookup off non-Windows keystrokes.
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
    // Why: legacy encoding collapses Ctrl+Enter to a bare CR, so TUIs that
    // expect modified Enter chords treat it as plain Enter. Forward the kitty
    // CSI-u sequence (modifier code 5 = Ctrl) so cue/queue behavior reaches the
    // TUI; with kitty/modifyOtherKeys negotiated the engine encoder already
    // emits the app's chosen form, so this stands down. A Windows fallback is
    // not added because no Windows TUI is known to drop this CSI-u form.
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
    // Why: Cmd+←/→ on macOS conventionally moves to start/end of line in
    // terminals (iTerm2, Ghostty). The engine has no default mapping for
    // Cmd+Arrow, so we translate to readline's Ctrl+A (\x01) / Ctrl+E (\x05),
    // which work universally across bash/zsh/fish and most TUI editors.
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
    // Why: macOS terminal users expect Cmd+↑/↓ to jump through scrollback
    // without writing escape bytes into the shell. Host action — legitimate
    // even for kitty apps, so deliberately NOT protocol-gated.
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
    // Why: the legacy encoding for option/alt+arrow is \e[1;3D / \e[1;3C, which
    // default readline (bash, zsh) does not bind to backward-word /
    // forward-word — so word navigation silently doesn't work without a custom
    // inputrc. Translate to \eb / \ef (readline's default word-nav bindings) so
    // option+←/→ on macOS and alt+←/→ on Linux/Windows behave like they do in
    // iTerm2's "Esc+" option-key mode. Platform-agnostic: both produce altKey.
    // Protocol-gated: a negotiated app wants the real modified-arrow report.
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
    // Why: local Windows ConPTY shells (PowerShell/cmd via PSReadLine) already
    // bind Ctrl+←/→ to word-nav, and they treat \eb/\ef (Alt+b/f) as
    // Escape→RevertLine followed by a self-inserted "b"/"f" — so the translation
    // below prints a stray letter instead of moving the cursor. Stand down and
    // let the engine encoder emit its native \e[1;5D / \e[1;5C there.
    // Remote/WSL panes on a Windows client run readline and still need the
    // translation, so this is gated on a genuine local native ConPTY, not
    // merely on the client being Windows.
    if (isLocalWindowsConptyPane?.()) {
      return null
    }
    // Why: default readline (bash, zsh) does not bind the legacy \e[1;5D /
    // \e[1;5C encoding for Ctrl+←/→, so Linux and remote/WSL shells need the
    // translation to \eb / \ef (same bytes as our Alt+Arrow rule) for word-nav
    // to work without a custom inputrc. Protocol-gated like the Alt+Arrow rule
    // above.
    //
    // Mac-gated: Ctrl+Arrow on macOS is reserved for Mission Control / Spaces
    // navigation at the OS level and should never reach the app.
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
