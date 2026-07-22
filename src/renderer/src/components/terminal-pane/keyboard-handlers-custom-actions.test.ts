/**
 * @vitest-environment happy-dom
 */
import { renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ResolvedCustomKeybinding } from '../../../../shared/custom-keybindings'
import type { PtyTransport } from './pty-transport'

const {
  recordTerminalUserInputForLeafMock,
  sendTerminalQuickCommandToPaneMock,
  copyTerminalTextVerifiedMock
} = vi.hoisted(() => ({
  recordTerminalUserInputForLeafMock: vi.fn(),
  sendTerminalQuickCommandToPaneMock: vi.fn(() => true),
  copyTerminalTextVerifiedMock: vi.fn(() => Promise.resolve(true))
}))

vi.mock('./terminal-input-activity', () => ({
  recordTerminalUserInputForLeaf: recordTerminalUserInputForLeafMock
}))

vi.mock('./terminal-quick-command-dispatch', () => ({
  sendTerminalQuickCommandToPane: sendTerminalQuickCommandToPaneMock
}))

vi.mock('./terminal-copy-outcome', () => ({
  copyTerminalTextVerified: copyTerminalTextVerifiedMock,
  reportTerminalCopyOutcome: vi.fn(),
  copyTerminalSelectionThenClear: vi.fn(),
  resetTerminalCopyOutcomeLatchesForTest: vi.fn()
}))

import { useTerminalKeyboardShortcuts } from './keyboard-handlers'
import { useAppStore } from '@/store'

function createPane(selection = '') {
  return {
    id: 1,
    leafId: 'leaf-1',
    terminal: {
      getSelection: () => selection,
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

// Cross-track composition pins (Wave-1 ownership waivers): 1B's custom-binding
// precedence and 1D's verified-copy pathway share keyboard-handlers.ts, and 1B's
// keyboard quick-command dispatch borders 1E's trust-gated project commands.
describe('useTerminalKeyboardShortcuts — custom bindings compose with the verified copy path', () => {
  beforeEach(() => {
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
    recordTerminalUserInputForLeafMock.mockClear()
    sendTerminalQuickCommandToPaneMock.mockClear()
    copyTerminalTextVerifiedMock.mockClear()
    useAppStore.setState({
      settings: {
        terminalQuickCommands: [{ id: 'qc-rebuild', name: 'rebuild', command: 'make' }]
      } as never
    })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const copyChordClash: ResolvedCustomKeybinding = {
    id: 'custom.copyclash001',
    title: 'Copy-chord macro',
    action: { type: 'sendText', text: 'CLASH' },
    bindings: ['Mod+Shift+C'],
    decodedText: 'CLASH'
  }

  it('same-chord custom sendText stays shadowed and the chord runs the verified copy seam', () => {
    const sendInput = vi.fn(() => true)
    const { unmount } = mountShortcuts(createPane('picked text'), sendInput, [copyChordClash])

    const event = dispatchKeydown({ key: 'c', code: 'KeyC', metaKey: true, shiftKey: true })

    // 1B precedence intact: the built-in wins the shared chord, custom bytes never sent.
    expect(sendInput).not.toHaveBeenCalled()
    // 1D pathway intact: the winning copySelection routes through the verified-copy seam.
    expect(copyTerminalTextVerifiedMock).toHaveBeenCalledWith('picked text', 'shortcut')
    expect(event.defaultPrevented).toBe(true)
    unmount()
  })

  it('empty selection declines the copy without falling through to the same-chord custom entry', () => {
    const sendInput = vi.fn(() => true)
    const { unmount } = mountShortcuts(createPane(''), sendInput, [copyChordClash])

    const event = dispatchKeydown({ key: 'c', code: 'KeyC', metaKey: true, shiftKey: true })

    // Why: the policy already resolved copySelection, so a no-selection decline must
    // leave the key unconsumed rather than re-enter custom dispatch.
    expect(copyTerminalTextVerifiedMock).not.toHaveBeenCalled()
    expect(sendInput).not.toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(false)
    unmount()
  })

  it('a custom binding naming a project (orcaYaml:) quick-command id fails closed on the keyboard path', () => {
    const projectQuickCommandEntry: ResolvedCustomKeybinding = {
      id: 'custom.projectqc01',
      title: 'Project deploy',
      action: { type: 'runQuickCommand', quickCommandId: 'orcaYaml:repo-1:0' },
      bindings: ['Mod+Alt+P']
    }
    const sendInput = vi.fn(() => true)
    const { unmount } = mountShortcuts(createPane(), sendInput, [projectQuickCommandEntry])

    const event = dispatchKeydown({ key: 'p', code: 'KeyP', metaKey: true, altKey: true })

    // Why: project commands live only in the per-repo hooks cache; the keyboard path
    // resolves ids against local Settings commands only, so repo-controlled orca.yaml
    // bytes can never reach a shell without TerminalPane's dispatch-time trust gate.
    expect(sendTerminalQuickCommandToPaneMock).not.toHaveBeenCalled()
    expect(sendInput).not.toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(true)
    unmount()
  })
})
