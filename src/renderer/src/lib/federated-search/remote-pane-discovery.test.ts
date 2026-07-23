import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { useAppStore } from '@/store'
import { makePaneKey } from '../../../../shared/stable-pane-id'
import type { ReplayedSearchGeometry } from '../../components/terminal-pane/pty-transport-types'
import { discoverRemoteFederatedPanes } from './remote-pane-discovery'
import {
  clearRemoteFederatedPaneBindings,
  registerRemoteFederatedPane,
  type RemoteFederatedPaneBinding
} from './remote-federated-pane-registry'

// Two real UUID leaf ids so makePaneKey (and the discovery join) run for real.
const LEAF_A = '11111111-1111-4111-8111-111111111111'
const LEAF_B = '22222222-2222-4222-8222-222222222222'

const GEOMETRY: ReplayedSearchGeometry = {
  replayedAnchor: { hostRowAnchor: 100, anchorGen: 7 },
  replayOriginRow: 0,
  replayedRowCount: 120,
  clientCols: 80
}

function bind(overrides: Partial<RemoteFederatedPaneBinding>): RemoteFederatedPaneBinding {
  return {
    tabId: 'tab-1',
    leafId: LEAF_A,
    environmentId: () => 'env-remote',
    hostTerminalId: () => 'host-term-9',
    sessionId: () => 'remote:env-remote@@host-term-9',
    replayGeometry: () => GEOMETRY,
    ...overrides
  }
}

function seedTabs(tabs: Record<string, { id: string; title: string | null }[]>): void {
  useAppStore.setState({ tabsByWorktree: tabs } as Parameters<typeof useAppStore.setState>[0])
}

describe('discoverRemoteFederatedPanes', () => {
  beforeEach(() => {
    clearRemoteFederatedPaneBindings()
    seedTabs({ 'wt-alpha': [{ id: 'tab-1', title: 'ssh: build' }] })
  })
  afterEach(() => {
    clearRemoteFederatedPaneBindings()
    seedTabs({})
  })

  it('enumerates a connected remote pane joined with store tab identity + real geometry', () => {
    registerRemoteFederatedPane(makePaneKey('tab-1', LEAF_A), bind({}))

    const panes = discoverRemoteFederatedPanes()
    expect(panes).toHaveLength(1)
    const [pane] = panes
    expect(pane.paneRef).toEqual({
      tabId: 'tab-1',
      leafId: LEAF_A,
      paneKey: `tab-1:${LEAF_A}`,
      worktreeId: 'wt-alpha',
      title: 'ssh: build'
    })
    expect(pane.environmentId).toBe('env-remote')
    expect(pane.hostTerminalId).toBe('host-term-9')
    expect(pane.sessionId).toBe('remote:env-remote@@host-term-9')
    // Geometry flows straight from the binding (the transport's frozen anchor).
    expect(pane.replayedAnchor).toEqual({ hostRowAnchor: 100, anchorGen: 7 })
    expect(pane.replayOriginRow).toBe(0)
    expect(pane.replayedRowCount).toBe(120)
    expect(pane.clientCols).toBe(80)
  })

  it('mutation guard: a wrong host / wrong anchor produces an observably different pane', () => {
    registerRemoteFederatedPane(
      makePaneKey('tab-1', LEAF_A),
      bind({
        hostTerminalId: () => 'WRONG-host',
        replayGeometry: () => ({ ...GEOMETRY, replayedAnchor: { hostRowAnchor: 999, anchorGen: 7 } })
      })
    )
    const [pane] = discoverRemoteFederatedPanes()
    // These are the values the remote wire request + row remap depend on; a
    // mutation here is exactly what would misroute the search or the jump.
    expect(pane.hostTerminalId).not.toBe('host-term-9')
    expect(pane.replayedAnchor?.hostRowAnchor).not.toBe(100)
  })

  it('skips a pane whose transport has not resolved its runtime/host yet (pre-connect)', () => {
    registerRemoteFederatedPane(
      makePaneKey('tab-1', LEAF_A),
      bind({ environmentId: () => null, hostTerminalId: () => null })
    )
    expect(discoverRemoteFederatedPanes()).toEqual([])
  })

  it('skips a pane whose tab is gone from the store (transport not yet disposed)', () => {
    registerRemoteFederatedPane(makePaneKey('tab-1', LEAF_A), bind({ tabId: 'tab-vanished' }))
    expect(discoverRemoteFederatedPanes()).toEqual([])
  })

  it('degrades to inline-only shape when no anchored snapshot is replayed (geometry null)', () => {
    registerRemoteFederatedPane(makePaneKey('tab-1', LEAF_A), bind({ replayGeometry: () => null }))
    const [pane] = discoverRemoteFederatedPanes()
    expect(pane.replayedAnchor).toBeNull()
    expect(pane.replayOriginRow).toBe(0)
    expect(pane.replayedRowCount).toBe(0)
    expect(pane.clientCols).toBeNull()
  })

  it('enumerates multiple remote panes across tabs', () => {
    seedTabs({
      'wt-alpha': [{ id: 'tab-1', title: 'a' }],
      'wt-beta': [{ id: 'tab-2', title: 'b' }]
    })
    registerRemoteFederatedPane(makePaneKey('tab-1', LEAF_A), bind({ tabId: 'tab-1', leafId: LEAF_A }))
    registerRemoteFederatedPane(
      makePaneKey('tab-2', LEAF_B),
      bind({ tabId: 'tab-2', leafId: LEAF_B, hostTerminalId: () => 'host-term-2' })
    )
    const keys = discoverRemoteFederatedPanes()
      .map((p) => p.paneRef?.paneKey)
      .sort()
    expect(keys).toEqual([`tab-1:${LEAF_A}`, `tab-2:${LEAF_B}`])
  })
})
