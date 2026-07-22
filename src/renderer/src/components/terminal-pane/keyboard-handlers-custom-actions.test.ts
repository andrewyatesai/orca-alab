/**
 * @vitest-environment happy-dom
 */
import { renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ResolvedCustomKeybinding } from '../../../../shared/custom-keybindings'
import type { PtyTransport } from './pty-transport'

const { recordTerminalUserInputForLeafMock, sendTerminalQuickCommandToPaneMock } = vi.hoisted(
  () => ({
    recordTerminalUserInputForLeafMock: vi.fn(),
    sendTerminalQuickCommandToPaneMock: vi.fn(() => true)
  })
)

vi.mock('./terminal-input-activity', () => ({
  recordTerminalUserInputForLeaf: recordTerminalUserInputForLeafMock
}))

vi.mock('./terminal-quick-command-dispatch', () => ({
  sendTerminalQuickCommandToPane: sendTerminalQuickCommandToPaneMock
}))

import { useTerminalKeyboardShortcuts } from './keyboard-handlers'
import { useAppStore } from '@/store'

function createPane() {
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

function mountShortcuts(
  pane: unknown,
  sendInput: ReturnType<typeof vi.fn>,
  customKeybindings: readonly ResolvedCustomKeybinding[]
) {
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
      onSearchSelectedText: noop,
      onRequestClosePane: noop,
      onClearPaneScrollback: noop,
      onSetTitle: noop,
      onClearPaneTitle: noop,
      searchOpenRef: { current: false },
      searchStateRef: { current: { query: '', caseSensitive: false, regex: false } },
      macOptionAsAltRef: { current: 'false' as const },
      customKeybindings
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

const sendTextEntry: ResolvedCustomKeybinding = {
  id: 'custom.macro0000001',
  title: 'Macro',
  action: { type: 'sendText', text: '\\x1b[13;2u' },
  bindings: ['Mod+Alt+M'],
  decodedText: '\x1b[13;2u'
}

const quickCommandEntry: ResolvedCustomKeybinding = {
  id: 'custom.quickcmd0001',
  title: 'Run rebuild',
  action: { type: 'runQuickCommand', quickCommandId: 'qc-rebuild' },
  bindings: ['Mod+Alt+B']
}

describe('useTerminalKeyboardShortcuts — custom keybinding dispatch', () => {
  beforeEach(() => {
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
    recordTerminalUserInputForLeafMock.mockClear()
    sendTerminalQuickCommandToPaneMock.mockClear()
    useAppStore.setState({
      settings: {
        terminalQuickCommands: [{ id: 'qc-rebuild', name: 'rebuild', command: 'make' }]
      } as never
    })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('custom sendInput writes decodedText to the transport and records leaf activity', () => {
    const sendInput = vi.fn(() => true)
    const { unmount } = mountShortcuts(createPane(), sendInput, [sendTextEntry])

    const event = dispatchKeydown({ key: 'm', code: 'KeyM', metaKey: true, altKey: true })

    expect(sendInput).toHaveBeenCalledWith('\x1b[13;2u')
    expect(recordTerminalUserInputForLeafMock).toHaveBeenCalledWith('tab-1', 'leaf-1')
    expect(event.defaultPrevented).toBe(true)
    unmount()
  })

  it('runQuickCommand resolves the command by id and dispatches via sendTerminalQuickCommandToPane', () => {
    const sendInput = vi.fn(() => true)
    const pane = createPane()
    const { unmount } = mountShortcuts(pane, sendInput, [quickCommandEntry])

    const event = dispatchKeydown({ key: 'b', code: 'KeyB', metaKey: true, altKey: true })

    expect(sendTerminalQuickCommandToPaneMock).toHaveBeenCalledTimes(1)
    expect(sendTerminalQuickCommandToPaneMock).toHaveBeenCalledWith(
      expect.objectContaining({
        command: expect.objectContaining({ id: 'qc-rebuild' }),
        pane,
        tabId: 'tab-1'
      })
    )
    expect(event.defaultPrevented).toBe(true)
    // The transport is only written by the dispatcher (mocked here), never directly.
    expect(sendInput).not.toHaveBeenCalled()
    unmount()
  })

  it('unknown quickCommandId no-ops but still consumes the chord', () => {
    const sendInput = vi.fn(() => true)
    useAppStore.setState({ settings: { terminalQuickCommands: [] } as never })
    const { unmount } = mountShortcuts(createPane(), sendInput, [quickCommandEntry])

    const event = dispatchKeydown({ key: 'b', code: 'KeyB', metaKey: true, altKey: true })

    expect(sendTerminalQuickCommandToPaneMock).not.toHaveBeenCalled()
    expect(sendInput).not.toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(true)
    unmount()
  })

  it('agent-prompt quick command no-ops when the dispatcher declines', () => {
    const sendInput = vi.fn(() => true)
    sendTerminalQuickCommandToPaneMock.mockReturnValueOnce(false)
    const { unmount } = mountShortcuts(createPane(), sendInput, [quickCommandEntry])

    dispatchKeydown({ key: 'b', code: 'KeyB', metaKey: true, altKey: true })

    expect(sendTerminalQuickCommandToPaneMock).toHaveBeenCalledTimes(1)
    expect(sendInput).not.toHaveBeenCalled()
    expect(recordTerminalUserInputForLeafMock).not.toHaveBeenCalled()
    unmount()
  })

  it('repeat of a runQuickCommand chord is swallowed without dispatching', () => {
    const sendInput = vi.fn(() => true)
    const { unmount } = mountShortcuts(createPane(), sendInput, [quickCommandEntry])

    const event = dispatchKeydown({
      key: 'b',
      code: 'KeyB',
      metaKey: true,
      altKey: true,
      repeat: true
    })

    expect(sendTerminalQuickCommandToPaneMock).not.toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(true)
    unmount()
  })
})
