import { describe, expect, it } from 'vitest'
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

describe('non-mac Ctrl+Left/Right word-nav', () => {
  // Linux and remote/WSL readline shells don't bind the legacy \e[1;5D /
  // \e[1;5C encoding, so translate to \eb / \ef (same bytes as our Alt+Arrow
  // rule).
  it('translates Ctrl+←/→ on Linux to readline \\eb / \\ef', () => {
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true }),
        false
      )
    ).toEqual({ type: 'sendInput', data: '\x1bb' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowRight', code: 'ArrowRight', ctrlKey: true }),
        false
      )
    ).toEqual({ type: 'sendInput', data: '\x1bf' })
  })

  // Local Windows ConPTY shells (PowerShell/cmd via PSReadLine) already bind
  // Ctrl+←/→ to word-nav and self-insert a stray "b"/"f" when fed \eb/\ef
  // (Escape→RevertLine + self-insert), so the policy must stand down and let
  // the engine emit its native \e[1;5D / \e[1;5C there. Signalled via the
  // isLocalWindowsConptyPane getter (7th arg).
  it('does NOT translate Ctrl+←/→ for a local Windows ConPTY pane', () => {
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true }),
        false,
        undefined,
        undefined,
        true,
        undefined,
        () => true
      )
    ).toBeNull()
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowRight', code: 'ArrowRight', ctrlKey: true }),
        false,
        undefined,
        undefined,
        true,
        undefined,
        () => true
      )
    ).toBeNull()
  })

  // A Windows client SSH'd into Linux (or running WSL) is NOT a local native
  // Windows ConPTY, so the getter returns false and the readline translation
  // must still apply — otherwise word-nav silently breaks on the remote shell.
  it('still translates Ctrl+←/→ on Windows when the pane is not local ConPTY (SSH/WSL)', () => {
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true }),
        false,
        undefined,
        undefined,
        true,
        undefined,
        () => false
      )
    ).toEqual({ type: 'sendInput', data: '\x1bb' })
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowRight', code: 'ArrowRight', ctrlKey: true }),
        false,
        undefined,
        undefined,
        true,
        undefined,
        () => false
      )
    ).toEqual({ type: 'sendInput', data: '\x1bf' })
  })

  it('does not translate Ctrl+Arrow on macOS (reserved by OS)', () => {
    // Mac uses Cmd+Arrow for line-nav and Option+Arrow for word-nav.
    // Ctrl+Arrow is the macOS Mission Control / Spaces chord.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true }),
        true
      )
    ).toBeNull()
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowRight', code: 'ArrowRight', ctrlKey: true }),
        true
      )
    ).toBeNull()
  })

  it('does not intercept Ctrl+Shift+Arrow (selection passthrough)', () => {
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true, shiftKey: true }),
        false
      )
    ).toBeNull()
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowRight', code: 'ArrowRight', ctrlKey: true, shiftKey: true }),
        false
      )
    ).toBeNull()
  })

  it('does not intercept Ctrl+Alt+Arrow (different chord)', () => {
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true, altKey: true }),
        false
      )
    ).toBeNull()
  })

  it('stands down under a negotiated kitty/modifyOtherKeys protocol', () => {
    // The app asked for real modified-key reports; the engine encoder speaks.
    expect(
      resolveTerminalShortcutAction(
        event({ key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true }),
        false,
        'false',
        0,
        false,
        undefined,
        undefined,
        () => true
      )
    ).toBeNull()
  })
})
