// End-to-end: a federated search that fans out across a REAL remote pane.
// Drives the actual path — the remote federated-pane registry (as the transport
// populates it) → discoverRemoteFederatedPanes (store join) → the remote source
// adapter's host-row → client-row remap over the 5B wire shape → the federation
// controller's merge/group. The `searchRemote` fixture behaves like the host:
// it answers only for the real host terminal and echoes its snapshot anchor ONLY
// for the generation the client actually replayed (terminal.ts:1186-1219). A
// mutation — wrong host, or a client anchor that disagrees with the host's — must
// stop a jumpable match from surfacing, never fabricate one.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useAppStore } from '@/store'
import { makePaneKey } from '../../../../shared/stable-pane-id'
import type { ReplayedSearchGeometry } from '../../components/terminal-pane/pty-transport-types'
import { createFederatedSearchController } from './federated-search-controller'
import { createRemotePaneSearchAdapter, type RemoteSearchCall } from './remote-pane-search-adapter'
import { discoverRemoteFederatedPanes } from './remote-pane-discovery'
import {
  clearRemoteFederatedPaneBindings,
  registerRemoteFederatedPane,
  type RemoteFederatedPaneBinding
} from './remote-federated-pane-registry'
import type { FederatedGroupOrderContext } from './federated-search-grouping'

const LEAF = '44444444-4444-4444-8444-444444444444'
const HOST_TERMINAL = 'host-term-9'

// The client replayed the host's gen-7 snapshot: its first row (host row 100) is
// client row 0, and it holds 120 rows at width 80.
const REPLAYED: ReplayedSearchGeometry = {
  replayedAnchor: { hostRowAnchor: 100, anchorGen: 7 },
  replayOriginRow: 0,
  replayedRowCount: 120,
  clientCols: 80
}

function registerRemotePane(overrides: Partial<RemoteFederatedPaneBinding> = {}): void {
  registerRemoteFederatedPane(makePaneKey('tab-1', LEAF), {
    tabId: 'tab-1',
    leafId: LEAF,
    environmentId: () => 'env-remote',
    hostTerminalId: () => HOST_TERMINAL,
    sessionId: () => 'remote:env-remote@@host-term-9',
    replayGeometry: () => REPLAYED,
    ...overrides
  })
}

// The host: two matches on host rows 110 and 150, echoing its gen-7 anchor only
// when the request names gen 7, and answering only for the real host terminal.
const hostSearch: RemoteSearchCall = async (pane, request) => {
  if (pane.hostTerminalId !== HOST_TERMINAL) {
    return {
      searchSchema: 1,
      available: false,
      matches: [],
      total: 0,
      incomplete: false,
      hostCols: 80
    }
  }
  const echo = request.clientAnchorGen === 7
  return {
    searchSchema: 1,
    available: true,
    matches: [
      { hostRow: 110, col: 0, len: 6, line: 'needle at host 110' },
      { hostRow: 150, col: 2, len: 6, line: 'a needle at host 150' }
    ],
    total: 2,
    incomplete: false,
    hostCols: 80,
    ...(echo ? { hostRowAnchor: 100, anchorGen: 7, anchorHostCols: 80 } : {})
  }
}

const ORDER: () => FederatedGroupOrderContext = () => ({
  focusedPaneKey: null,
  visiblePaneKeys: new Set<string>(),
  outputRecency: () => 0
})

function seedTabs(tabs: Record<string, { id: string; title: string | null }[]>): void {
  useAppStore.setState({ tabsByWorktree: tabs } as Parameters<typeof useAppStore.setState>[0])
}

function buildController(searchRemote: RemoteSearchCall) {
  return createFederatedSearchController({
    adapters: [
      createRemotePaneSearchAdapter({
        discoverRemotePanes: discoverRemoteFederatedPanes,
        searchRemote
      })
    ],
    orderContext: ORDER
  })
}

describe('federated search across a remote pane (end-to-end)', () => {
  beforeEach(() => {
    clearRemoteFederatedPaneBindings()
    seedTabs({ 'wt-alpha': [{ id: 'tab-1', title: 'ssh: build' }] })
  })
  afterEach(() => {
    clearRemoteFederatedPaneBindings()
    seedTabs({})
  })

  it('fans out to the remote pane and surfaces host matches remapped into client rows', async () => {
    registerRemotePane()
    const controller = buildController(hostSearch)
    controller.setQuery('needle', { caseSensitive: false, isRegex: false })
    await vi.waitFor(() => expect(controller.snapshot().pending).toBe(false))

    const { groups } = controller.snapshot()
    expect(groups).toHaveLength(1)
    const [group] = groups
    expect(group.source).toBe('remote')
    expect(group.sessionId).toBe('remote:env-remote@@host-term-9')
    expect(group.paneRef?.paneKey).toBe(`tab-1:${LEAF}`)
    // host 150 → client 50, host 110 → client 10; newest-first.
    expect(group.matches.map((m) => m.absRow)).toEqual([50, 10])
    expect(group.matches[0]).toMatchObject({ absRow: 50, snippet: 'a needle at host 150' })
    expect(group.incomplete).toBe(false)
    controller.dispose()
  })

  it('mutation — a client replayed-anchor that disagrees with the host degrades to inline-only, never a wrong jump', async () => {
    // The client thinks it replayed host row 999 for gen 7; the host still echoes
    // its real anchor (host row 100 / gen 7). The generations match but the rows
    // do not → anchor-mismatch → no jumpable match, honestly incomplete.
    registerRemotePane({
      replayGeometry: () => ({ ...REPLAYED, replayedAnchor: { hostRowAnchor: 999, anchorGen: 7 } })
    })
    const controller = buildController(hostSearch)
    controller.setQuery('needle', { caseSensitive: false, isRegex: false })
    await vi.waitFor(() => expect(controller.snapshot().pending).toBe(false))

    const [group] = controller.snapshot().groups
    expect(group.matches).toEqual([])
    expect(group.total).toBe(2) // honest count of host matches
    expect(group.incomplete).toBe(true) // both need inline expansion, not a jump
    controller.dispose()
  })

  it('mutation — a wrong host terminal id yields no remote group (source absent, not a bad answer)', async () => {
    registerRemotePane({ hostTerminalId: () => 'WRONG-host' })
    const controller = buildController(hostSearch)
    controller.setQuery('needle', { caseSensitive: false, isRegex: false })
    await vi.waitFor(() => expect(controller.snapshot().pending).toBe(false))
    expect(controller.snapshot().groups).toEqual([])
    controller.dispose()
  })

  it('carries in-window and deeper-history matches honestly when only some are jumpable', async () => {
    registerRemotePane()
    const mixedHost: RemoteSearchCall = async (pane, request) => {
      const base = await hostSearch(pane, request, new AbortController().signal)
      if (!base || !base.available) {
        return base
      }
      return {
        ...base,
        matches: [
          { hostRow: 110, col: 0, len: 6, line: 'in-window' }, // client 10
          { hostRow: 50, col: 0, len: 6, line: 'older than the replayed window' } // out-of-window
        ],
        total: 2
      }
    }
    const controller = buildController(mixedHost)
    controller.setQuery('needle', { caseSensitive: false, isRegex: false })
    await vi.waitFor(() => expect(controller.snapshot().pending).toBe(false))

    const [group] = controller.snapshot().groups
    expect(group.matches.map((m) => m.absRow)).toEqual([10])
    expect(group.incomplete).toBe(true)
    controller.dispose()
  })
})
