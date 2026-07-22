/**
 * @vitest-environment happy-dom
 *
 * Wave-1 ownership-waiver composition pins for TerminalPane.tsx (1E's
 * dispatch-time project-command trust gate landed beside 1B's
 * customKeybindings threading). These tests mount the REAL TerminalPane so
 * both cross-track edits are exercised at their actual composition point:
 * - 1B: store customKeybindings reach the real useTerminalKeyboardShortcuts
 *   through TerminalPane's wiring and still dispatch to the pane transport.
 * - 1E: the same mounted component's onQuickCommand prop (the trust gate)
 *   fails closed for untrusted orca.yaml commands and passes through only
 *   when the repo's trust record covers the shared-content hash.
 */
import { render, cleanup } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ResolvedCustomKeybinding } from '../../../../shared/custom-keybindings'
import type { TerminalQuickCommand } from '../../../../shared/types'

const {
  paneFixture,
  contextMenuFixture,
  capturedContextMenuProps,
  reviewProjectQuickCommandTrustMock,
  projectQuickCommandsStateHolder,
  sendInputMock,
  recordTerminalUserInputForLeafMock
} = vi.hoisted(() => {
  const sendInputMock = vi.fn(() => true)
  // Why: makePaneKey validates leaf ids as UUIDs.
  const LEAF_ID = '11111111-1111-4111-8111-111111111111'
  const pane = {
    id: 1,
    leafId: LEAF_ID,
    terminal: {
      getSelection: () => '',
      focus: vi.fn(),
      element: document.createElement('div'),
      modes: { bracketedPasteMode: false },
      cols: 80,
      rows: 24
    },
    container: document.createElement('div'),
    atermController: null
  }
  const transport = {
    sendInput: sendInputMock,
    getPtyId: () => 'pty-1',
    isConnected: () => true,
    resize: vi.fn(),
    destroy: vi.fn()
  }
  const manager = {
    getPanes: () => [pane],
    getActivePane: () => pane,
    setActivePane: vi.fn(),
    getNumericIdForLeaf: () => 1,
    beginPaneDragFromPointerDown: vi.fn()
  }
  return {
    paneFixture: { pane, transport, manager, leafId: LEAF_ID },
    contextMenuFixture: {
      open: false,
      setOpen: vi.fn(),
      point: { x: 0, y: 0 },
      menuOpenedAtRef: { current: 0 },
      paneCount: 1,
      menuPaneId: null,
      onContextMenuCapture: vi.fn(),
      onPaneTitleContextMenu: vi.fn(),
      onCopy: vi.fn(async () => undefined),
      onCopyTerminalId: vi.fn(async () => undefined),
      onCopyPaneId: vi.fn(),
      onPaste: vi.fn(async () => undefined),
      onSplitRight: vi.fn(),
      onSplitDown: vi.fn(),
      onEqualizePaneSizes: vi.fn(),
      onClosePane: vi.fn(),
      onClearScreen: vi.fn(),
      onForkAgentSession: vi.fn(async () => undefined),
      onCopyAgentSessionContext: vi.fn(async () => undefined),
      onQuickCommand: vi.fn(),
      onToggleExpand: vi.fn(),
      onSetTitle: vi.fn(),
      onClearPaneTitle: vi.fn(),
      runForPane: vi.fn()
    },
    capturedContextMenuProps: { current: null as Record<string, unknown> | null },
    reviewProjectQuickCommandTrustMock: vi.fn(() => Promise.resolve(undefined)),
    projectQuickCommandsStateHolder: {
      current: {
        commands: [] as TerminalQuickCommand[],
        sharedTrustContentHash: null as string | null,
        trusted: false
      }
    },
    sendInputMock,
    recordTerminalUserInputForLeafMock: vi.fn()
  }
})

// Why: PTY/xterm creation is 1A/2B territory the pin doesn't own; the mock
// hydrates the same refs the production lifecycle fills so the REAL keyboard
// wiring and trust gate run against a live-looking pane.
vi.mock('./use-terminal-pane-lifecycle', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return {
    ...actual,
    useTerminalPaneLifecycle: (args: {
      managerRef: { current: unknown }
      paneTransportsRef: { current: Map<number, unknown> }
    }) => {
      args.managerRef.current = paneFixture.manager
      if (!args.paneTransportsRef.current.has(paneFixture.pane.id)) {
        args.paneTransportsRef.current.set(paneFixture.pane.id, paneFixture.transport)
      }
    }
  }
})

vi.mock('./use-terminal-pane-global-effects', () => ({
  useTerminalPaneGlobalEffects: () => undefined
}))

vi.mock('./use-terminal-pane-context-menu', () => ({
  useTerminalPaneContextMenu: () => contextMenuFixture
}))

vi.mock('@/hooks/use-project-quick-commands', () => ({
  useProjectQuickCommands: () => projectQuickCommandsStateHolder.current,
  reviewProjectQuickCommandTrust: reviewProjectQuickCommandTrustMock
}))

vi.mock('./terminal-input-activity', () => ({
  recordTerminalUserInputForLeaf: recordTerminalUserInputForLeafMock
}))

// Presentation-only children: strip them so the mount exercises TerminalPane's
// wiring without pulling dialog/portal UI into the fixture.
vi.mock('./TerminalContextMenu', () => ({
  default: (props: Record<string, unknown>) => {
    capturedContextMenuProps.current = props
    return null
  }
}))
vi.mock('./TerminalPaneHeaderOverlay', () => ({ default: () => null }))
vi.mock('@/components/TerminalSearch', () => ({ default: () => null }))
vi.mock('../native-chat/NativeChatView', () => ({ default: () => null }))
vi.mock('./CloseTerminalDialog', () => ({ default: () => null }))
vi.mock('./MobileDriverOverlay', () => ({ MobileDriverOverlay: () => null }))
vi.mock('./TerminalErrorToast', () => ({ TerminalErrorToast: () => null }))
vi.mock('./TerminalDeadPaneOverlay', () => ({ TerminalDeadPaneOverlay: () => null }))
vi.mock('./TerminalSessionStateSaveFailureDialog', () => ({
  TerminalSessionStateSaveFailureDialog: () => null
}))
vi.mock('./TerminalAgentSessionForkDialog', () => ({
  TerminalAgentSessionForkDialog: () => null
}))
vi.mock('./SessionRestoredBannerPortals', () => ({ SessionRestoredBannerPortals: () => null }))
vi.mock('./TerminalSshReconnectOverlay', () => ({ TerminalSshReconnectOverlay: () => null }))
vi.mock('@/components/terminal-quick-commands/TerminalQuickCommandDialog', () => ({
  TerminalQuickCommandDialog: () => null,
  createTerminalQuickCommandDraft: (scope: unknown) => ({
    id: 'draft-1',
    label: '',
    command: '',
    appendEnter: true,
    scope
  })
}))
vi.mock('@/components/shared/useDaemonActions', () => ({
  useDaemonActions: () => ({
    pending: null,
    busyKind: null,
    setPending: vi.fn(),
    confirm: vi.fn(),
    cancel: vi.fn()
  }),
  DaemonActionDialog: () => null
}))
vi.mock('@/components/link-routing-preference-dialog', () => ({
  useLinkRoutingPreferenceDialog: () => vi.fn()
}))

import TerminalPane from './TerminalPane'
import { useAppStore } from '@/store'
import { getRepoIdFromWorktreeId } from '../../../../shared/worktree-id'

const WORKTREE_ID = 'repo-1::/tmp/repo-wt'
const REPO_ID = getRepoIdFromWorktreeId(WORKTREE_ID)
const SHARED_TRUST_HASH = 'shared-hash-1'

const macroBinding: ResolvedCustomKeybinding = {
  id: 'custom.macro0000001',
  title: 'Macro',
  action: { type: 'sendText', text: '\\x1b[13;2u' },
  bindings: ['Mod+Alt+M'],
  decodedText: '\x1b[13;2u'
}

const projectQuickCommand: TerminalQuickCommand = {
  id: `orcaYaml:${REPO_ID}:0`,
  label: 'Project deploy',
  action: 'terminal-command',
  command: 'make deploy',
  appendEnter: true,
  scope: { type: 'repo', repoId: REPO_ID }
}

function renderTerminalPane(): ReturnType<typeof render> {
  return render(
    <TerminalPane
      tabId="tab-1"
      worktreeId={WORKTREE_ID}
      cwd="/tmp/repo-wt"
      isActive
      isVisible
      isWorktreeActive
      isolatedPaneKey={null}
      showSplitButton
      onPtyExit={() => undefined}
      onCloseTab={() => undefined}
    />
  )
}

function dispatchScopedKeydown(
  overrides: Partial<KeyboardEventInit> & { key: string }
): KeyboardEvent {
  const container = document.querySelector('[data-terminal-tab-id="tab-1"]')
  if (!(container instanceof HTMLElement)) {
    throw new Error('TerminalPane container did not mount')
  }
  // Why: the keyboard hook scopes events to the pane container; dispatch from
  // a child so the scope check sees a target inside this TerminalPane.
  const origin = document.createElement('span')
  container.appendChild(origin)
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...overrides })
  origin.dispatchEvent(event)
  origin.remove()
  return event
}

describe('TerminalPane composition — 1B customKeybindings wiring beside 1E trust gate', () => {
  beforeEach(() => {
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
    // Why: calls must work as subscriptions too (effects return the result as
    // a cleanup), so apply yields the callable, non-thenable proxy itself.
    const apiProxy: unknown = new Proxy(function () {}, {
      get: (_target, prop) => (prop === 'then' ? undefined : apiProxy),
      apply: () => apiProxy
    })
    ;(window as unknown as { api: unknown }).api = apiProxy
    sendInputMock.mockClear()
    recordTerminalUserInputForLeafMock.mockClear()
    reviewProjectQuickCommandTrustMock.mockClear()
    contextMenuFixture.onQuickCommand.mockClear()
    capturedContextMenuProps.current = null
    projectQuickCommandsStateHolder.current = {
      commands: [projectQuickCommand],
      sharedTrustContentHash: SHARED_TRUST_HASH,
      trusted: false
    }
    useAppStore.setState({
      settings: { terminalQuickCommands: [] },
      customKeybindings: [macroBinding],
      trustedOrcaHooks: {},
      repos: [
        { id: REPO_ID, path: '/tmp/repo', displayName: 'Repo', badgeColor: 'blue', addedAt: 1 }
      ]
    } as never)
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('threads store customKeybindings through TerminalPane into the real keyboard hook (1B survives)', () => {
    renderTerminalPane()

    const event = dispatchScopedKeydown({ key: 'm', code: 'KeyM', metaKey: true, altKey: true })

    expect(sendInputMock).toHaveBeenCalledWith('\x1b[13;2u')
    expect(recordTerminalUserInputForLeafMock).toHaveBeenCalledWith('tab-1', paneFixture.leafId)
    expect(event.defaultPrevented).toBe(true)
  })

  it('fails closed for untrusted project quick commands at the mounted dispatch gate (1E)', () => {
    renderTerminalPane()

    const onQuickCommand = capturedContextMenuProps.current?.onQuickCommand as (
      command: TerminalQuickCommand
    ) => void
    expect(onQuickCommand).toBeTypeOf('function')

    act(() => onQuickCommand(projectQuickCommand))

    // Untrusted orca.yaml bytes never reach dispatch; the gate re-opens review.
    expect(contextMenuFixture.onQuickCommand).not.toHaveBeenCalled()
    expect(reviewProjectQuickCommandTrustMock).toHaveBeenCalledWith(REPO_ID)
  })

  it('passes trusted project commands through and keeps the custom keybinding path live in the same mount', () => {
    renderTerminalPane()

    useAppStore.setState({
      trustedOrcaHooks: { [REPO_ID]: { setup: { contentHash: SHARED_TRUST_HASH } } }
    } as never)

    const onQuickCommand = capturedContextMenuProps.current?.onQuickCommand as (
      command: TerminalQuickCommand
    ) => void
    act(() => onQuickCommand(projectQuickCommand))

    expect(contextMenuFixture.onQuickCommand).toHaveBeenCalledWith(projectQuickCommand)
    expect(reviewProjectQuickCommandTrustMock).not.toHaveBeenCalled()

    // Both cross-track edits coexist: the trust gate just ran, and 1B's
    // keyboard dispatch still works in the same mounted TerminalPane.
    dispatchScopedKeydown({ key: 'm', code: 'KeyM', metaKey: true, altKey: true })
    expect(sendInputMock).toHaveBeenCalledWith('\x1b[13;2u')
  })

  it('non-project quick commands bypass the trust gate without touching review', () => {
    renderTerminalPane()

    const localCommand: TerminalQuickCommand = {
      id: 'qc-local',
      label: 'rebuild',
      action: 'terminal-command',
      command: 'make',
      appendEnter: true
    }
    const onQuickCommand = capturedContextMenuProps.current?.onQuickCommand as (
      command: TerminalQuickCommand
    ) => void
    act(() => onQuickCommand(localCommand))

    expect(contextMenuFixture.onQuickCommand).toHaveBeenCalledWith(localCommand)
    expect(reviewProjectQuickCommandTrustMock).not.toHaveBeenCalled()
  })
})
