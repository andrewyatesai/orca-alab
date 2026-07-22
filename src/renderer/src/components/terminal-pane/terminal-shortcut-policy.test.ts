import { describe, expect, it, vi } from 'vitest'
import { createTerminalNativeOnlyShortcutTracker } from './terminal-native-only-shortcut'
import {
  resolveTerminalShortcutAction,
  type TerminalShortcutEvent
} from './terminal-shortcut-policy'

function event(overrides: Partial<TerminalShortcutEvent>): TerminalShortcutEvent {
  return {
    key: '',
    code: '',
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    repeat: false,
    ...overrides
  }
}

describe('resolveTerminalShortcutAction', () => {
  it('resolves Mod+Shift+Period to toggleComposeBox once per press', () => {
    const chord = event({ key: '>', code: 'Period', metaKey: true, shiftKey: true })
    expect(resolveTerminalShortcutAction(chord, true)).toEqual({ type: 'toggleComposeBox' })
    // Repeats never re-toggle (matches the other once-per-press pane commands).
    expect(resolveTerminalShortcutAction({ ...chord, repeat: true }, true)).toBeNull()
    expect(
      resolveTerminalShortcutAction(
        event({ key: '>', code: 'Period', ctrlKey: true, shiftKey: true }),
        false
      )
    ).toEqual({ type: 'toggleComposeBox' })
  })

  it('preserves macOS readline ctrl chords for the shell', () => {
    const passthroughCases = [
      event({ key: 'r', code: 'KeyR', ctrlKey: true }),
      event({ key: 'u', code: 'KeyU', ctrlKey: true }),
      event({ key: 'e', code: 'KeyE', ctrlKey: true }),
      event({ key: 'a', code: 'KeyA', ctrlKey: true }),
      event({ key: 'w', code: 'KeyW', ctrlKey: true }),
      event({ key: 'k', code: 'KeyK', ctrlKey: true })
    ]

    for (const input of passthroughCases) {
      expect(resolveTerminalShortcutAction(input, true)).toBeNull()
    }
  })

  it('resolves the explicit macOS terminal shortcut allowlist', () => {
    expect(
      resolveTerminalShortcutAction(event({ key: 'f', code: 'KeyF', metaKey: true }), true)
    ).toEqual({
      type: 'toggleSearch'
    })
    expect(
      resolveTerminalShortcutAction(event({ key: 'k', code: 'KeyK', metaKey: true }), true)
    ).toEqual({
      type: 'clearActivePane'
    })
    expect(
      resolveTerminalShortcutAction(event({ key: 'w', code: 'KeyW', metaKey: true }), true)
    ).toEqual({
      type: 'closeActivePane'
    })
    expect(
      resolveTerminalShortcutAction(event({ key: 'd', code: 'KeyD', metaKey: true }), true)
    ).toEqual({ type: 'splitActivePane', direction: 'vertical' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'd', code: 'KeyD', metaKey: true, shiftKey: true }),
        true
      )
    ).toEqual({ type: 'splitActivePane', direction: 'horizontal' })
    expect(
      resolveTerminalShortcutAction(event({ key: '[', code: 'BracketLeft', metaKey: true }), true)
    ).toEqual({ type: 'focusPane', direction: 'previous' })
    expect(
      resolveTerminalShortcutAction(event({ key: ']', code: 'BracketRight', metaKey: true }), true)
    ).toEqual({ type: 'focusPane', direction: 'next' })
  })

  it('keeps inactive shift-enter and delete helpers explicit', () => {
    expect(
      resolveTerminalShortcutAction(event({ key: 'Enter', code: 'Enter', shiftKey: true }), true)
    ).toEqual({
      type: 'sendInput',
      data: '\x1b\r'
    })
    expect(resolveTerminalShortcutAction(event({ key: 'Backspace', ctrlKey: true }), true)).toEqual(
      { type: 'sendInput', data: '\x17' }
    )
    expect(resolveTerminalShortcutAction(event({ key: 'Backspace', metaKey: true }), true)).toEqual(
      { type: 'sendInput', data: '\x15' }
    )
    expect(resolveTerminalShortcutAction(event({ key: 'Delete', metaKey: true }), true)).toEqual({
      type: 'sendInput',
      data: '\x0b'
    })
    expect(resolveTerminalShortcutAction(event({ key: 'Backspace', altKey: true }), true)).toEqual({
      type: 'sendInput',
      data: '\x1b\x7f'
    })
  })

  // The Windows Shift+Enter cases exercise the upstream host/agent byte routing.
  // The positional callbacks are isLocalWindowsConptyPane (7),
  // isKittyKeyboardActivePane (8), layoutBaseCharacterForCode (9),
  // getWindowsShiftEnterEncoding (10), and isWindowsTerminalHost (11).
  it('uses the Codex-compatible Shift+Enter sequence on Windows win32-input-mode panes', () => {
    // Default and explicit legacy encodings both keep Codex-on-PowerShell
    // newlining instead of ignoring the chord.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', shiftKey: true }),
        false,
        'false',
        0,
        true
      )
    ).toEqual({
      type: 'sendInput',
      data: '\x1b\r'
    })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', shiftKey: true }),
        false,
        'false',
        0,
        true,
        undefined,
        undefined,
        undefined,
        undefined,
        () => 'alt-enter'
      )
    ).toEqual({ type: 'sendInput', data: '\x1b\r' })
  })

  it('sends CSI-u Shift+Enter to Windows panes whose active agent requires it (#7620)', () => {
    // Why: droid parses CSI-u directly and treats the Alt+Enter byte as a plain
    // Enter that submits, so its pane capability must produce `\x1b[13;2u`.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', shiftKey: true }),
        false,
        'false',
        0,
        true,
        undefined,
        undefined,
        undefined,
        undefined,
        () => 'csi-u'
      )
    ).toEqual({ type: 'sendInput', data: '\x1b[13;2u' })
  })

  it('uses CSI-u for a non-Windows PTY reached from Windows only while Kitty is active', () => {
    const getWindowsShiftEnterEncoding = vi.fn(() => 'csi-u' as const)
    const resolve = (kittyActive: boolean) =>
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', shiftKey: true }),
        false,
        'false',
        0,
        true,
        undefined,
        undefined,
        () => kittyActive,
        undefined,
        getWindowsShiftEnterEncoding,
        () => false
      )
    expect(resolve(true)).toEqual({ type: 'sendInput', data: '\x1b[13;2u' })
    expect(resolve(false)).toEqual({ type: 'sendInput', data: '\x1b\r' })
    expect(getWindowsShiftEnterEncoding).not.toHaveBeenCalled()
  })

  it('uses CSI-u Shift+Enter off Windows only while Kitty keyboard is active', () => {
    for (const encoding of [() => 'csi-u' as const, () => 'alt-enter' as const, undefined]) {
      const resolve = (kittyActive: boolean) =>
        resolveTerminalShortcutAction(
          event({ key: 'Enter', code: 'Enter', shiftKey: true }),
          false,
          'false',
          0,
          false,
          undefined,
          undefined,
          () => kittyActive,
          undefined,
          encoding
        )
      expect(resolve(true)).toEqual({ type: 'sendInput', data: '\x1b[13;2u' })
      expect(resolve(false)).toEqual({ type: 'sendInput', data: '\x1b\r' })
    }
  })

  it('keeps host and agent lookups off unrelated keystrokes', () => {
    const isLocalWindowsConptyPane = vi.fn(() => true)
    const isKittyKeyboardActivePane = vi.fn(() => true)
    const getWindowsShiftEnterEncoding = vi.fn(() => 'csi-u' as const)
    const isWindowsTerminalHost = vi.fn(() => true)

    expect(
      resolveTerminalShortcutAction(
        event({ key: 'a', code: 'KeyA' }),
        false,
        'false',
        0,
        true,
        undefined,
        isLocalWindowsConptyPane,
        isKittyKeyboardActivePane,
        undefined,
        getWindowsShiftEnterEncoding,
        isWindowsTerminalHost
      )
    ).toBeNull()
    expect(isLocalWindowsConptyPane).not.toHaveBeenCalled()
    expect(getWindowsShiftEnterEncoding).not.toHaveBeenCalled()
    expect(isWindowsTerminalHost).not.toHaveBeenCalled()
    expect(isKittyKeyboardActivePane).not.toHaveBeenCalled()

    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', shiftKey: true }),
        false,
        'false',
        0,
        true,
        undefined,
        isLocalWindowsConptyPane,
        isKittyKeyboardActivePane,
        undefined,
        getWindowsShiftEnterEncoding,
        isWindowsTerminalHost
      )
    ).toEqual({ type: 'sendInput', data: '\x1b[13;2u' })
    expect(isLocalWindowsConptyPane).not.toHaveBeenCalled()
    expect(getWindowsShiftEnterEncoding).toHaveBeenCalledTimes(1)
    expect(isWindowsTerminalHost).toHaveBeenCalledTimes(1)
    expect(isKittyKeyboardActivePane).not.toHaveBeenCalled()

    isWindowsTerminalHost.mockReturnValue(false)
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', shiftKey: true }),
        false,
        'false',
        0,
        true,
        undefined,
        isLocalWindowsConptyPane,
        isKittyKeyboardActivePane,
        undefined,
        getWindowsShiftEnterEncoding,
        isWindowsTerminalHost
      )
    ).toEqual({ type: 'sendInput', data: '\x1b[13;2u' })
    expect(isLocalWindowsConptyPane).not.toHaveBeenCalled()
    expect(getWindowsShiftEnterEncoding).toHaveBeenCalledTimes(1)
    expect(isWindowsTerminalHost).toHaveBeenCalledTimes(2)
    expect(isKittyKeyboardActivePane).toHaveBeenCalledTimes(1)
  })

  it('honors Kitty negotiation for a Windows PTY reached from macOS', () => {
    const getWindowsShiftEnterEncoding = vi.fn(() => 'alt-enter' as const)
    const resolve = (kittyActive: boolean) =>
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', shiftKey: true }),
        true,
        'false',
        0,
        false,
        undefined,
        undefined,
        () => kittyActive,
        undefined,
        getWindowsShiftEnterEncoding,
        () => true
      )
    expect(resolve(true)).toEqual({ type: 'sendInput', data: '\x1b[13;2u' })
    expect(resolve(false)).toEqual({ type: 'sendInput', data: '\x1b\r' })
    expect(getWindowsShiftEnterEncoding).toHaveBeenCalledTimes(2)
  })

  describe('kitty/modifyOtherKeys negotiated (kittyKeyboardActive)', () => {
    // Signature: (event, isMac, macOptionAsAlt, optionKeyLocation, isWindows,
    // keybindings, isLocalWindowsConptyPane, isKittyKeyboardActivePane).
    const resolveNegotiated = (
      e: TerminalShortcutEvent,
      isMac: boolean,
      macOptionAsAlt: Parameters<typeof resolveTerminalShortcutAction>[2] = 'false',
      optionKeyLocation = 0
    ) =>
      resolveTerminalShortcutAction(
        e,
        isMac,
        macOptionAsAlt,
        optionKeyLocation,
        false,
        undefined,
        undefined,
        () => true
      )

    it('stands the Ctrl+Backspace readline rewrite down (engine emits \\x1b[127;5u)', () => {
      const chord = event({ key: 'Backspace', ctrlKey: true })
      expect(resolveNegotiated(chord, true)).toBeNull()
      expect(resolveNegotiated(chord, false)).toBeNull()
      // Un-negotiated panes keep the legacy delete-word byte.
      expect(resolveTerminalShortcutAction(chord, true)).toEqual({
        type: 'sendInput',
        data: '\x17'
      })
    })

    it('stands the Alt+Backspace readline rewrite down (engine emits \\x1b[127;3u)', () => {
      const chord = event({ key: 'Backspace', altKey: true })
      expect(resolveNegotiated(chord, true)).toBeNull()
      expect(resolveTerminalShortcutAction(chord, true)).toEqual({
        type: 'sendInput',
        data: '\x1b\x7f'
      })
    })

    it('routes Cmd+Backspace/Delete/←/→ through the engine as SUPER chords (never dead)', () => {
      // Why not a silent stand-down: the aterm keydown encoder hard-nulls ALL
      // metaKey events, so these chords would go dead without the encodeKey
      // action. The fallback is the legacy byte so an engine miss still works.
      expect(resolveNegotiated(event({ key: 'Backspace', metaKey: true }), true)).toEqual({
        type: 'encodeKey',
        key: 'Backspace',
        mods: { super: true },
        fallback: '\x15'
      })
      expect(resolveNegotiated(event({ key: 'Delete', metaKey: true }), true)).toEqual({
        type: 'encodeKey',
        key: 'Delete',
        mods: { super: true },
        fallback: '\x0b'
      })
      expect(
        resolveNegotiated(event({ key: 'ArrowLeft', code: 'ArrowLeft', metaKey: true }), true)
      ).toEqual({ type: 'encodeKey', key: 'ArrowLeft', mods: { super: true }, fallback: '\x01' })
      expect(
        resolveNegotiated(event({ key: 'ArrowRight', code: 'ArrowRight', metaKey: true }), true)
      ).toEqual({ type: 'encodeKey', key: 'ArrowRight', mods: { super: true }, fallback: '\x05' })
    })

    it('keeps Cmd+↑/↓ as host scrollback navigation even when negotiated', () => {
      expect(
        resolveNegotiated(event({ key: 'ArrowUp', code: 'ArrowUp', metaKey: true }), true)
      ).toEqual({ type: 'scrollViewport', position: 'top' })
      expect(
        resolveNegotiated(event({ key: 'ArrowDown', code: 'ArrowDown', metaKey: true }), true)
      ).toEqual({ type: 'scrollViewport', position: 'bottom' })
    })

    it('routes the macOS Option+B/F/D compensation through the engine as ALT chords', () => {
      // The ENGINE picks the dialect from live mode bits (kitty CSI-u vs xterm
      // modifyOtherKeys) — the policy must not hard-code either form.
      expect(resolveNegotiated(event({ key: '∫', code: 'KeyB', altKey: true }), true)).toEqual({
        type: 'encodeKey',
        key: 'b',
        mods: { alt: true },
        fallback: '\x1bb'
      })
      expect(resolveNegotiated(event({ key: 'ƒ', code: 'KeyF', altKey: true }), true)).toEqual({
        type: 'encodeKey',
        key: 'f',
        mods: { alt: true },
        fallback: '\x1bf'
      })
      expect(resolveNegotiated(event({ key: '∂', code: 'KeyD', altKey: true }), true)).toEqual({
        type: 'encodeKey',
        key: 'd',
        mods: { alt: true },
        fallback: '\x1bd'
      })
    })

    it('routes designated Option-as-meta letters through the engine as ALT chords', () => {
      // Left Option acting as Meta (macOptionAsAlt='left', location=1).
      expect(
        resolveNegotiated(event({ key: '˜', code: 'KeyN', altKey: true }), true, 'left', 1)
      ).toEqual({ type: 'encodeKey', key: 'n', mods: { alt: true }, fallback: '\x1bn' })
      expect(
        resolveNegotiated(event({ key: '¡', code: 'Digit1', altKey: true }), true, 'left', 1)
      ).toEqual({ type: 'encodeKey', key: '1', mods: { alt: true }, fallback: '\x1b1' })
      // Un-negotiated: the legacy Esc+letter rewrite is preserved.
      expect(
        resolveTerminalShortcutAction(
          event({ key: '˜', code: 'KeyN', altKey: true }),
          true,
          'left',
          1
        )
      ).toEqual({ type: 'sendInput', data: '\x1bn' })
    })
  })

  it('forwards Ctrl+Enter as the kitty CSI-u chord so TUIs can cue instead of send', () => {
    // Why: legacy encoding collapses Ctrl+Enter to a bare CR; intercept upstream
    // and emit the kitty sequence (modifier code 5 = Ctrl) so probing TUIs
    // receive the distinct chord on every platform.
    expect(
      resolveTerminalShortcutAction(event({ key: 'Enter', code: 'Enter', ctrlKey: true }), true)
    ).toEqual({ type: 'sendInput', data: '\x1b[13;5u' })
    expect(
      resolveTerminalShortcutAction(event({ key: 'Enter', code: 'Enter', ctrlKey: true }), false)
    ).toEqual({ type: 'sendInput', data: '\x1b[13;5u' })
    // Windows uses the same kitty sequence for now: no TUI is known to treat the
    // CSI-u Ctrl+Enter form as inert (cf. the Shift+Enter Codex-on-PowerShell case).
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', ctrlKey: true }),
        false,
        'false',
        0,
        true
      )
    ).toEqual({ type: 'sendInput', data: '\x1b[13;5u' })

    // Modifier combos that are NOT plain Ctrl+Enter must keep falling through.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', ctrlKey: true, shiftKey: true }),
        true
      )
    ).toBeNull()
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', ctrlKey: true, metaKey: true }),
        true
      )
    ).toBeNull()

    // With kitty/modifyOtherKeys negotiated the app wants the engine's real
    // chord, so the rewrite stands down.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'Enter', code: 'Enter', ctrlKey: true }),
        true,
        'false',
        0,
        false,
        undefined,
        undefined,
        () => true
      )
    ).toBeNull()
  })

  it('translates Cmd+←/→ on macOS to readline start/end-of-line (Ctrl+A/E)', () => {
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', metaKey: true }),
        true
      )
    ).toEqual({ type: 'sendInput', data: '\x01' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowRight', code: 'ArrowRight', metaKey: true }),
        true
      )
    ).toEqual({ type: 'sendInput', data: '\x05' })

    // Cmd+Shift+Arrow is a different chord (selection) — don't intercept.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', metaKey: true, shiftKey: true }),
        true
      )
    ).toBeNull()
  })

  it('maps Cmd+↑/↓ on macOS to terminal scrollback top/bottom navigation', () => {
    expect(
      resolveTerminalShortcutAction(event({ key: 'ArrowUp', code: 'ArrowUp', metaKey: true }), true)
    ).toEqual({ type: 'scrollViewport', position: 'top' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowDown', code: 'ArrowDown', metaKey: true }),
        true
      )
    ).toEqual({ type: 'scrollViewport', position: 'bottom' })

    // Cmd+Shift+Arrow is selection territory; leave it to focused apps/shells.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowUp', code: 'ArrowUp', metaKey: true, shiftKey: true }),
        true
      )
    ).toBeNull()
  })

  it('preserves existing non-Mac terminal pane shortcuts', () => {
    expect(
      resolveTerminalShortcutAction(event({ key: 'f', code: 'KeyF', ctrlKey: true }), false)
    ).toEqual({ type: 'toggleSearch' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'c', code: 'KeyC', ctrlKey: true, shiftKey: true }),
        false
      )
    ).toEqual({ type: 'copySelection' })
    expect(
      resolveTerminalShortcutAction(event({ key: 'r', code: 'KeyR', ctrlKey: true }), false)
    ).toBeNull()
    expect(
      resolveTerminalShortcutAction(event({ key: 'k', code: 'KeyK', ctrlKey: true }), false)
    ).toEqual({ type: 'clearActivePane' })
    expect(
      resolveTerminalShortcutAction(event({ key: 'w', code: 'KeyW', ctrlKey: true }), false)
    ).toEqual({ type: 'closeActivePane' })
  })

  it('applies custom terminal pane keybindings', () => {
    const keybindings = {
      'terminal.clear': ['Ctrl+Alt+K'],
      'terminal.search': []
    }

    expect(
      resolveTerminalShortcutAction(
        event({ key: 'k', code: 'KeyK', ctrlKey: true, shiftKey: true }),
        false,
        'false',
        0,
        false,
        keybindings
      )
    ).toBeNull()
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'k', code: 'KeyK', ctrlKey: true, altKey: true }),
        false,
        'false',
        0,
        false,
        keybindings
      )
    ).toEqual({ type: 'clearActivePane' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'f', code: 'KeyF', ctrlKey: true }),
        false,
        'false',
        0,
        false,
        keybindings
      )
    ).toBeNull()
  })

  it('resolves equalize pane sizes only when users assign it', () => {
    expect(
      resolveTerminalShortcutAction(event({ key: '=', code: 'Equal', metaKey: true }), true)
    ).toBeNull()
    expect(
      resolveTerminalShortcutAction(
        event({ key: '=', code: 'Equal', metaKey: true }),
        true,
        'false',
        0,
        false,
        { 'terminal.equalizePaneSizes': ['Mod+Equal'] }
      )
    ).toEqual({ type: 'equalizePaneSizes' })
  })

  it('resolves terminal title actions only when users assign them', () => {
    expect(
      resolveTerminalShortcutAction(event({ key: 't', code: 'KeyT', metaKey: true }), true)
    ).toBeNull()
    expect(
      resolveTerminalShortcutAction(
        event({ key: 't', code: 'KeyT', metaKey: true }),
        true,
        'false',
        0,
        false,
        { 'terminal.setTitle': ['Mod+T'] }
      )
    ).toEqual({ type: 'setTitle' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 't', code: 'KeyT', metaKey: true, altKey: true }),
        true,
        'false',
        0,
        false,
        { 'terminal.clearPaneTitle': ['Mod+Alt+T'] }
      )
    ).toEqual({ type: 'clearPaneTitle' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 't', code: 'KeyT', metaKey: true, altKey: true, repeat: true }),
        true,
        'false',
        0,
        false,
        { 'terminal.clearPaneTitle': ['Mod+Alt+T'] }
      )
    ).toBeNull()
  })

  it('lets Ctrl+D pass through as EOF on non-Mac, requires Shift for split (#586)', () => {
    // Ctrl+D without Shift on Windows/Linux must NOT trigger split — it's EOF
    expect(
      resolveTerminalShortcutAction(event({ key: 'd', code: 'KeyD', ctrlKey: true }), false)
    ).toBeNull()

    // Ctrl+Shift+D on Windows/Linux splits the pane right (vertical)
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'd', code: 'KeyD', ctrlKey: true, shiftKey: true }),
        false
      )
    ).toEqual({ type: 'splitActivePane', direction: 'vertical' })

    // Alt+Shift+D on Windows/Linux splits the pane down (horizontal)
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'd', code: 'KeyD', altKey: true, shiftKey: true }),
        false
      )
    ).toEqual({ type: 'splitActivePane', direction: 'horizontal' })

    // Alt+Shift+D should NOT trigger split-down on Mac (Mac uses Cmd+Shift+D)
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'd', code: 'KeyD', altKey: true, shiftKey: true }),
        true
      )
    ).toBeNull()

    // Alt+D (no Shift) on Windows/Linux must pass through for readline forward-word-delete
    expect(
      resolveTerminalShortcutAction(event({ key: 'd', code: 'KeyD', altKey: true }), false)
    ).toBeNull()
  })

  it('translates alt+arrow to readline word-nav escapes on both platforms', () => {
    // macOS: option+←/→ → \eb / \ef (readline backward-word / forward-word)
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', altKey: true }),
        true
      )
    ).toEqual({ type: 'sendInput', data: '\x1bb' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowRight', code: 'ArrowRight', altKey: true }),
        true
      )
    ).toEqual({ type: 'sendInput', data: '\x1bf' })

    // Linux/Windows: alt+←/→ produces the same escapes (platform-agnostic chord)
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', altKey: true }),
        false
      )
    ).toEqual({ type: 'sendInput', data: '\x1bb' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowRight', code: 'ArrowRight', altKey: true }),
        false
      )
    ).toEqual({ type: 'sendInput', data: '\x1bf' })

    // alt+shift+arrow is a different chord (select-word in some shells) — don't
    // intercept, let the shell handle it.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', altKey: true, shiftKey: true }),
        true
      )
    ).toBeNull()

    // alt+ctrl+arrow is a different chord entirely — passthrough.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', altKey: true, ctrlKey: true }),
        true
      )
    ).toBeNull()

    // Ctrl+Alt+Arrow (Linux workspace switching on some desktops) must pass through on non-Mac.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true, altKey: true }),
        false
      )
    ).toBeNull()

    // Regression guard: plain ArrowLeft must still pass through untouched.
    expect(
      resolveTerminalShortcutAction(event({ key: 'ArrowLeft', code: 'ArrowLeft' }), true)
    ).toBeNull()

    // Kitty-gated: a negotiated app gets the real alt+arrow report from the
    // engine encoder, not the readline-compat rewrite.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', altKey: true }),
        true,
        'false',
        0,
        false,
        undefined,
        undefined,
        () => true
      )
    ).toBeNull()
  })

  it('translates macOS Option+B/F/D to readline escape sequences in compose mode', () => {
    // With macOptionAsAlt='false' (compose), the legacy path doesn't translate
    // these. Matches on event.code because macOS composition replaces event.key.
    expect(
      resolveTerminalShortcutAction(event({ key: '∫', code: 'KeyB', altKey: true }), true, 'false')
    ).toEqual({ type: 'sendInput', data: '\x1bb' })
    expect(
      resolveTerminalShortcutAction(event({ key: 'ƒ', code: 'KeyF', altKey: true }), true, 'false')
    ).toEqual({ type: 'sendInput', data: '\x1bf' })
    expect(
      resolveTerminalShortcutAction(event({ key: '∂', code: 'KeyD', altKey: true }), true, 'false')
    ).toEqual({ type: 'sendInput', data: '\x1bd' })

    // On Linux/Windows, Alt+B/F/D must still pass through
    expect(
      resolveTerminalShortcutAction(event({ key: 'b', code: 'KeyB', altKey: true }), false)
    ).toBeNull()

    // Option+Shift+B/F/D should not be intercepted (different chord)
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'B', code: 'KeyB', altKey: true, shiftKey: true }),
        true,
        'false'
      )
    ).toBeNull()
  })

  it('sends Esc+letter for any Option+letter when left Option acts as alt', () => {
    // Left Option (optionKeyLocation=1) in 'left' mode: full Meta for any letter key
    expect(
      resolveTerminalShortcutAction(
        event({ key: '¬', code: 'KeyL', altKey: true }),
        true,
        'left',
        1
      )
    ).toEqual({ type: 'sendInput', data: '\x1bl' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: '†', code: 'KeyT', altKey: true }),
        true,
        'left',
        1
      )
    ).toEqual({ type: 'sendInput', data: '\x1bt' })

    // Right Option (optionKeyLocation=2) in 'left' mode: compose side, only B/F/D patched
    expect(
      resolveTerminalShortcutAction(
        event({ key: '∫', code: 'KeyB', altKey: true }),
        true,
        'left',
        2
      )
    ).toEqual({ type: 'sendInput', data: '\x1bb' })
    // Right Option+L should pass through (compose character)
    expect(
      resolveTerminalShortcutAction(
        event({ key: '¬', code: 'KeyL', altKey: true }),
        true,
        'left',
        2
      )
    ).toBeNull()
  })

  it('sends Esc+letter for any Option+letter when right Option acts as alt', () => {
    // Right Option (optionKeyLocation=2) in 'right' mode: full Meta, including punctuation
    expect(
      resolveTerminalShortcutAction(
        event({ key: '≥', code: 'Period', altKey: true }),
        true,
        'right',
        2
      )
    ).toEqual({ type: 'sendInput', data: '\x1b.' })

    expect(
      resolveTerminalShortcutAction(
        event({ key: '¬', code: 'KeyL', altKey: true }),
        true,
        'right',
        2
      )
    ).toEqual({ type: 'sendInput', data: '\x1bl' })

    // Left Option (optionKeyLocation=1) in 'right' mode: compose side, only B/F/D patched
    expect(
      resolveTerminalShortcutAction(
        event({ key: '¬', code: 'KeyL', altKey: true }),
        true,
        'right',
        1
      )
    ).toBeNull()
  })

  it('does not intercept Option+letter in true mode (xterm handles it)', () => {
    // In 'true' mode, macOptionIsMeta is enabled in xterm, so no compensation needed
    // Our handler still fires but is gated by macOptionAsAlt !== 'true'
    expect(
      resolveTerminalShortcutAction(event({ key: 'b', code: 'KeyB', altKey: true }), true, 'true')
    ).toBeNull()
  })

  it('keeps Cmd+D and Cmd+Shift+D for split on macOS', () => {
    expect(
      resolveTerminalShortcutAction(event({ key: 'd', code: 'KeyD', metaKey: true }), true)
    ).toEqual({ type: 'splitActivePane', direction: 'vertical' })

    expect(
      resolveTerminalShortcutAction(
        event({ key: 'd', code: 'KeyD', metaKey: true, shiftKey: true }),
        true
      )
    ).toEqual({ type: 'splitActivePane', direction: 'horizontal' })
  })

  it('resolves terminal.switchInputSource via explicit override (for OS input-source chords)', () => {
    // Why: the configured chord must route to the native-only handler rather
    // than the terminal shortcut paths that cancel the browser default.
    const overrides = { 'terminal.switchInputSource': ['Shift+Space'] }
    expect(
      resolveTerminalShortcutAction(
        event({ key: ' ', code: 'Space', shiftKey: true }),
        true,
        'false',
        0,
        false,
        overrides
      )
    ).toEqual({ type: 'switchInputSource' })

    const otherChord = { 'terminal.switchInputSource': ['Ctrl+Space'] }
    expect(
      resolveTerminalShortcutAction(
        event({ key: ' ', code: 'Space', ctrlKey: true }),
        false,
        'false',
        0,
        false,
        otherChord
      )
    ).toEqual({ type: 'switchInputSource' })
  })

  it('does not resolve switchInputSource for ordinary chords without override', () => {
    expect(
      resolveTerminalShortcutAction(event({ key: ' ', code: 'Space', shiftKey: true }), true)
    ).toBeNull()
  })

  it('suppresses the full companion event sequence for a native-only shortcut', () => {
    const tracker = createTerminalNativeOnlyShortcutTracker()
    tracker.armKeyDown(event({ key: ' ', code: 'Space', shiftKey: true }))

    expect(tracker.consumeCompanion({ type: 'keypress', key: ' ', code: 'Space' })).toBe(true)
    expect(tracker.consumeCompanion({ type: 'keyup', key: ' ', code: 'Space' })).toBe(true)
    expect(tracker.consumeCompanion({ type: 'keyup', key: ' ', code: 'Space' })).toBe(false)
  })
})
