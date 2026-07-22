// @vitest-environment happy-dom

// Wave-gate IME repro: the compose-box toggle chord pressed mid-composition
// must defer the open until the pending glyph commits (deferred-newline
// interplay), instead of stealing focus from the IME helper textarea.

import { renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useTerminalKeyboardShortcuts } from './keyboard-handlers'
import type { PtyTransport } from './pty-transport'
import { useAppStore } from '@/store'

function createPane(): {
  id: number
  leafId: string
  terminal: { getSelection: () => string; focus: () => void; element: HTMLElement }
  atermController: null
} {
  return {
    id: 1,
    leafId: 'leaf-1',
    terminal: {
      getSelection: () => '',
      focus: vi.fn(),
      element: document.createElement('div')
    },
    atermController: null
  }
}

function mountShortcuts(pane: unknown, onToggleComposeBox: () => void) {
  const transports = new Map<number, PtyTransport>([
    [1, { sendInput: vi.fn(() => true) } as unknown as PtyTransport]
  ])
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
      onToggleComposeBox,
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

function composeChord(overrides: KeyboardEventInit & { isComposing?: boolean } = {}): KeyboardEvent {
  const { isComposing, ...init } = overrides
  const event = new KeyboardEvent('keydown', {
    key: '.',
    code: 'Period',
    metaKey: true,
    shiftKey: true,
    bubbles: true,
    cancelable: true,
    ...init
  })
  if (isComposing) {
    // Why: happy-dom's KeyboardEventInit may drop isComposing; pin the real mid-IME event shape.
    Object.defineProperty(event, 'isComposing', { value: true })
  }
  return event
}

async function flushMacrotasks(count = 3): Promise<void> {
  for (let index = 0; index < count; index += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
}

describe('terminal.composeBox keyboard handling', () => {
  let previousSettings: unknown

  beforeEach(() => {
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
    previousSettings = useAppStore.getState().settings
  })

  afterEach(() => {
    useAppStore.setState({ settings: previousSettings as never })
    vi.unstubAllGlobals()
  })

  it('opens the compose box on Mod+Shift+Period and consumes the chord', () => {
    const onToggleComposeBox = vi.fn()
    const pane = createPane()
    const { unmount } = mountShortcuts(pane, onToggleComposeBox)

    const event = composeChord()
    window.dispatchEvent(event)

    expect(onToggleComposeBox).toHaveBeenCalledTimes(1)
    expect(event.defaultPrevented).toBe(true)
    unmount()
  })

  it('toggleComposeBox chord mid-composition defers the open until compositionend', async () => {
    const onToggleComposeBox = vi.fn()
    const pane = createPane()
    const { unmount } = mountShortcuts(pane, onToggleComposeBox)

    window.dispatchEvent(composeChord({ isComposing: true }))
    expect(onToggleComposeBox).not.toHaveBeenCalled()

    // The pending glyph commits into the terminal, then the box opens clean.
    pane.terminal.element.dispatchEvent(new Event('compositionend'))
    await flushMacrotasks()

    expect(onToggleComposeBox).toHaveBeenCalledTimes(1)
    unmount()
  })

  it('toggleComposeBox is inert when settings.terminalComposeBox is false', () => {
    useAppStore.setState({ settings: { terminalComposeBox: false } as never })
    const onToggleComposeBox = vi.fn()
    const pane = createPane()
    const { unmount } = mountShortcuts(pane, onToggleComposeBox)

    const event = composeChord()
    window.dispatchEvent(event)

    expect(onToggleComposeBox).not.toHaveBeenCalled()
    // Why: inert means unconsumed — the chord falls through as if unbound.
    expect(event.defaultPrevented).toBe(false)
    unmount()
  })
})
