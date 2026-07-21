import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ParsedAgentStatusPayload } from '../../../shared/agent-status-types'

// Why: split from agent-hook-completion-notifications.test.ts — the pane
// liveness / coordinator pruning lifecycle is a separate concern and the main
// suite is at the max-lines budget.

const dispatchTerminalNotification = vi.fn()
const dispatchAgentHookTerminalLifecycle = vi.fn()

type MockStoreState = {
  settings: {
    experimentalTerminalAttention?: boolean
    notifications: {
      enabled: boolean
      agentTaskComplete: boolean
    }
  }
  ptyIdsByTabId: Record<string, string[]>
  suppressedPtyExitIds: Record<string, boolean>
  tabsByWorktree: Record<string, { id: string; ptyId?: string | null }[]>
  terminalLayoutsByTabId: Record<
    string,
    {
      root: { type: 'leaf'; leafId: string } | null
      activeLeafId: string | null
      expandedLeafId: string | null
      ptyIdsByLeafId?: Record<string, string>
    }
  >
  agentLaunchConfigByPaneKey: Record<
    string,
    {
      launchConfig: { agentArgs: string; agentEnv: Record<string, string> }
      launchToken?: string
    }
  >
  agentStatusByPaneKey: Record<string, never>
  getAgentLaunchConfigForStatusEntry: (entry: {
    paneKey: string
  }) => { agentArgs: string; agentEnv: Record<string, string> } | undefined
  getAgentLaunchConfigForStatusMetadata: (metadata: {
    paneKey: string
    launchToken?: string
  }) => { agentArgs: string; agentEnv: Record<string, string> } | undefined
}

let mockStoreState: MockStoreState

vi.mock('@/store', () => ({
  useAppStore: {
    getState: () => mockStoreState
  }
}))

vi.mock('@/components/terminal-pane/use-notification-dispatch', () => ({
  dispatchTerminalNotification
}))

vi.mock('@/components/terminal-pane/agent-hook-terminal-lifecycle', () => ({
  dispatchAgentHookTerminalLifecycle
}))

function hookStatus(state: ParsedAgentStatusPayload['state']): ParsedAgentStatusPayload {
  return {
    state,
    prompt: 'implement notifications',
    agentType: 'codex'
  }
}

describe('agent hook completion notification coordinator pruning', () => {
  const paneKey = 'tab-1:11111111-1111-4111-8111-111111111111'

  beforeEach(() => {
    vi.resetModules()
    vi.useFakeTimers()
    dispatchTerminalNotification.mockClear()
    dispatchAgentHookTerminalLifecycle.mockClear()
    mockStoreState = {
      settings: {
        experimentalTerminalAttention: false,
        notifications: {
          enabled: true,
          agentTaskComplete: true
        }
      },
      ptyIdsByTabId: {
        'tab-1': ['pty-1']
      },
      suppressedPtyExitIds: {},
      tabsByWorktree: {
        'wt-1': [{ id: 'tab-1', ptyId: 'pty-1' }]
      },
      terminalLayoutsByTabId: {},
      agentLaunchConfigByPaneKey: {},
      agentStatusByPaneKey: {},
      getAgentLaunchConfigForStatusEntry: (entry) =>
        mockStoreState.agentLaunchConfigByPaneKey[entry.paneKey]?.launchConfig,
      getAgentLaunchConfigForStatusMetadata: (metadata) =>
        metadata.launchToken &&
        metadata.launchToken ===
          mockStoreState.agentLaunchConfigByPaneKey[metadata.paneKey]?.launchToken
          ? mockStoreState.agentLaunchConfigByPaneKey[metadata.paneKey]?.launchConfig
          : undefined
    }
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  const MANY_PANES = [
    { tabId: 'tab-1', leafId: '11111111-1111-4111-8111-111111111111', ptyId: 'pty-1' },
    { tabId: 'tab-2', leafId: '22222222-2222-4222-8222-222222222222', ptyId: 'pty-2' },
    { tabId: 'tab-3', leafId: '33333333-3333-4333-8333-333333333333', ptyId: 'pty-3' },
    { tabId: 'tab-4', leafId: '44444444-4444-4444-8444-444444444444', ptyId: 'pty-4' },
    { tabId: 'tab-5', leafId: '55555555-5555-4555-8555-555555555555', ptyId: 'pty-5' }
  ]

  function seedManyLivePanes(): void {
    mockStoreState.ptyIdsByTabId = Object.fromEntries(MANY_PANES.map((p) => [p.tabId, [p.ptyId]]))
    mockStoreState.tabsByWorktree = {
      'wt-1': MANY_PANES.map((p) => ({ id: p.tabId, ptyId: p.ptyId }))
    }
  }

  it('prunes only the coordinators whose panes lost liveness, keeping the rest', async () => {
    seedManyLivePanes()
    const {
      _getAgentHookCompletionNotificationCoordinatorCountForTest,
      observeAgentHookCompletionForNotification,
      syncAgentHookCompletionNotificationSettings
    } = await import('./agent-hook-completion-notifications')

    for (const pane of MANY_PANES) {
      observeAgentHookCompletionForNotification({
        paneKey: `${pane.tabId}:${pane.leafId}`,
        worktreeId: 'wt-1',
        payload: hookStatus('working')
      })
    }
    expect(_getAgentHookCompletionNotificationCoordinatorCountForTest()).toBe(MANY_PANES.length)

    // Remove liveness for two panes (both the tab hint and the pty list).
    mockStoreState.tabsByWorktree = {
      'wt-1': MANY_PANES.slice(0, 3).map((p) => ({ id: p.tabId, ptyId: p.ptyId }))
    }
    mockStoreState.ptyIdsByTabId = Object.fromEntries(
      MANY_PANES.slice(0, 3).map((p) => [p.tabId, [p.ptyId]])
    )
    syncAgentHookCompletionNotificationSettings()

    expect(_getAgentHookCompletionNotificationCoordinatorCountForTest()).toBe(3)
  })

  it('gates cosmetic store updates but still prunes after a pane closes', async () => {
    const {
      _getAgentHookCompletionNotificationCoordinatorCountForTest,
      observeAgentHookCompletionForNotification,
      syncAgentHookCompletionNotificationsForStoreUpdate
    } = await import('./agent-hook-completion-notifications')

    observeAgentHookCompletionForNotification({
      paneKey,
      worktreeId: 'wt-1',
      payload: hookStatus('working')
    })

    const beforeCosmeticUpdate = { ...mockStoreState }
    mockStoreState.tabsByWorktree = {
      'wt-1': [{ id: 'tab-1', ptyId: 'pty-1' }]
    }
    expect(
      syncAgentHookCompletionNotificationsForStoreUpdate(mockStoreState, beforeCosmeticUpdate)
    ).toBe(false)
    expect(_getAgentHookCompletionNotificationCoordinatorCountForTest()).toBe(1)

    const beforeClose = { ...mockStoreState }
    mockStoreState.tabsByWorktree = { 'wt-1': [] }
    mockStoreState.ptyIdsByTabId = {}
    expect(syncAgentHookCompletionNotificationsForStoreUpdate(mockStoreState, beforeClose)).toBe(
      true
    )
    expect(_getAgentHookCompletionNotificationCoordinatorCountForTest()).toBe(0)
  })

  it('skips tab scans until a pane-liveness slice changes', async () => {
    seedManyLivePanes()
    const {
      observeAgentHookCompletionForNotification,
      syncAgentHookCompletionNotificationSettings
    } = await import('./agent-hook-completion-notifications')

    for (const pane of MANY_PANES) {
      observeAgentHookCompletionForNotification({
        paneKey: `${pane.tabId}:${pane.leafId}`,
        worktreeId: 'wt-1',
        payload: hookStatus('working')
      })
    }

    // Count full tab-map enumerations rather than cheap reference reads.
    const realTabs = mockStoreState.tabsByWorktree
    let tabEnumerationCount = 0
    mockStoreState.tabsByWorktree = new Proxy(realTabs, {
      ownKeys(target) {
        tabEnumerationCount += 1
        return Reflect.ownKeys(target)
      }
    })

    syncAgentHookCompletionNotificationSettings()

    expect(tabEnumerationCount).toBe(1)

    syncAgentHookCompletionNotificationSettings()

    expect(tabEnumerationCount).toBe(1)

    mockStoreState.ptyIdsByTabId = { ...mockStoreState.ptyIdsByTabId }
    syncAgentHookCompletionNotificationSettings()

    expect(tabEnumerationCount).toBe(2)
  })
})
