/**
 * @vitest-environment happy-dom
 */
import { renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useTerminalKeyboardShortcuts } from './keyboard-handlers'
import type { PtyTransport } from './pty-transport'

// The window-level 'encodeKey' dispatch: the shortcut policy emits it for
// kitty-negotiated panes (Cmd chords behind the encoder's metaKey firewall,
// macOS Option composition); the handler must encode through the ACTIVE
// pane's engine and send via the pane transport — falling back to the
// action's legacy bytes when the engine returns nothing.

function createPane(args: {
  keyboardModeBits: number
  encodeKeyForHost: (key: string, mods: number) => string | null
}) {
  return {
    id: 1,
    leafId: 'leaf-1',
    terminal: {
      getSelection: () => '',
      focus: vi.fn()
    },
    atermController: {
      keyboardModeBits: () => args.keyboardModeBits,
      encodeKeyForHost: args.encodeKeyForHost
    }
  }
}

function mountShortcuts(pane: unknown, sendInput: ReturnType<typeof vi.fn>) {
  const transports = new Map<number, PtyTransport>([[1, { sendInput } as unknown as PtyTransport]])
  const noop = (): void => undefined
  return renderHook(() =>
    useTerminalKeyboardShortcuts({
      tabId: 'tab-1',
      worktreeId: 'wt-1',
      isActive: true,
      keyboardScopeRef: { current: null },
      managerRef: {
        current: {
          getActivePane: () => pane,
          getPanes: () => [pane]
        } as never
      },
      paneTransportsRef: { current: transports },
      paneCwdRef: { current: new Map() },
      fallbackCwd: '/tmp',
      expandedPaneIdRef: { current: null },
      setExpandedPane: noop,
      restoreExpandedLayout: noop,
      refreshPaneSizes: noop,
      persistLayoutSnapshot: noop,
      toggleExpandPane: noop,
      setSearchOpen: noop as never,
      onToggleComposeBox: noop,
      onSearchSelectedText: noop,
      onRequestClosePane: noop,
      onClearPaneScrollback: noop,
      onSetTitle: noop,
      onClearPaneTitle: noop,
      searchOpenRef: { current: false },
      searchStateRef: { current: { query: '', caseSensitive: false, regex: false } },
      macOptionAsAltRef: { current: 'false' as const }
    })
  )
}

function dispatchKeydown(overrides: Partial<KeyboardEventInit> & { key: string }): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    bubbles: true,
    cancelable: true,
    ...overrides
  })
  window.dispatchEvent(event)
  return event
}

describe('useTerminalKeyboardShortcuts — encodeKey action dispatch', () => {
  beforeEach(() => {
    // The policy's Cmd-chord branch is macOS-only; isMac reads the UA.
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('encodes Cmd+Backspace through the pane engine and sends the negotiated bytes', () => {
    const sendInput = vi.fn(() => true)
    const encodeKeyForHost = vi.fn(() => '\x1b[127;9u') // kitty SUPER+Backspace
    const pane = createPane({ keyboardModeBits: 0x1, encodeKeyForHost })
    const hook = mountShortcuts(pane, sendInput)

    const event = dispatchKeydown({ key: 'Backspace', metaKey: true })

    // SUPER = 8 (engine Modifiers bitfield).
    expect(encodeKeyForHost).toHaveBeenCalledWith('Backspace', 8)
    expect(sendInput).toHaveBeenCalledWith('\x1b[127;9u')
    expect(event.defaultPrevented).toBe(true)
    hook.unmount()
  })

  it('falls back to the legacy bytes when the engine has no encoding (never dead)', () => {
    const sendInput = vi.fn(() => true)
    const encodeKeyForHost = vi.fn(() => null)
    const pane = createPane({ keyboardModeBits: 0x1, encodeKeyForHost })
    const hook = mountShortcuts(pane, sendInput)

    dispatchKeydown({ key: 'Backspace', metaKey: true })

    expect(sendInput).toHaveBeenCalledWith('\x15')
    hook.unmount()
  })

  it('sends the legacy rewrite directly when the pane has NOT negotiated a key protocol', () => {
    const sendInput = vi.fn(() => true)
    const encodeKeyForHost = vi.fn(() => '\x1b[127;9u')
    const pane = createPane({ keyboardModeBits: 0, encodeKeyForHost })
    const hook = mountShortcuts(pane, sendInput)

    dispatchKeydown({ key: 'Backspace', metaKey: true })

    // Un-negotiated: the policy resolves the plain sendInput rewrite; the
    // engine encoder is never consulted.
    expect(encodeKeyForHost).not.toHaveBeenCalled()
    expect(sendInput).toHaveBeenCalledWith('\x15')
    hook.unmount()
  })
})
