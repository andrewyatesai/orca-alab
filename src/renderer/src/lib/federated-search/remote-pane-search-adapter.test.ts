import { describe, expect, it, vi } from 'vitest'
import {
  createRemotePaneSearchAdapter,
  type DiscoveredRemotePane,
  type RemoteSearchCall
} from './remote-pane-search-adapter'
import type { RemoteTerminalSearchResult } from '../../../../shared/terminal-remote-search-protocol'
import type { FederatedPaneBatch } from './federated-search-model'

function remotePane(overrides?: Partial<DiscoveredRemotePane>): DiscoveredRemotePane {
  return {
    paneRef: { tabId: 't1', leafId: 'a', paneKey: 't1:a', worktreeId: 'wt', title: 'ssh' },
    sessionId: 's-remote',
    replayedAnchor: { hostRowAnchor: 100, anchorGen: 5 },
    replayOriginRow: 0,
    replayedRowCount: 100,
    clientCols: 80,
    lastOutputAt: 42,
    environmentId: 'env-1',
    hostTerminalId: 'host-term-1',
    ...overrides
  }
}

function result(overrides?: Partial<RemoteTerminalSearchResult>): RemoteTerminalSearchResult {
  return {
    searchSchema: 1,
    available: true,
    matches: [],
    total: 0,
    incomplete: false,
    hostCols: 80,
    hostRowAnchor: 100,
    anchorGen: 5,
    anchorHostCols: 80,
    ...overrides
  }
}

async function collect(
  adapter: ReturnType<typeof createRemotePaneSearchAdapter>,
  gen = 1
): Promise<FederatedPaneBatch[]> {
  const batches: FederatedPaneBatch[] = []
  await adapter.query('needle', { caseSensitive: false, isRegex: false }, gen, 50, (b) =>
    batches.push(b)
  )
  return batches
}

describe('createRemotePaneSearchAdapter', () => {
  it('remaps host rows to client rows and emits a remote batch (newest-first)', async () => {
    const searchRemote: RemoteSearchCall = vi.fn(async () =>
      result({
        matches: [
          { hostRow: 110, col: 0, len: 6, line: 'needle at 110' },
          { hostRow: 150, col: 2, len: 6, line: 'a needle at 150' }
        ],
        total: 2
      })
    )
    const adapter = createRemotePaneSearchAdapter({
      discoverRemotePanes: () => [remotePane()],
      searchRemote
    })
    const [batch] = await collect(adapter)
    expect(batch.source).toBe('remote')
    expect(batch.sessionId).toBe('s-remote')
    // client row = replayOrigin(0) + hostRow − anchor(100). Newest-first: 50, 10.
    expect(batch.matches.map((m) => m.absRow)).toEqual([50, 10])
    expect(batch.matches[0]).toMatchObject({ absRow: 50, snippet: 'a needle at 150' })
    expect(batch.total).toBe(2)
    expect(batch.incomplete).toBe(false)
  })

  it('passes the replayed anchorGen so the host only echoes THAT snapshot', async () => {
    const searchRemote = vi.fn<RemoteSearchCall>(async () => result())
    const adapter = createRemotePaneSearchAdapter({
      discoverRemotePanes: () => [remotePane({ replayedAnchor: { hostRowAnchor: 100, anchorGen: 9 } })],
      searchRemote
    })
    await collect(adapter, 7)
    expect(searchRemote).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ gen: 7, clientAnchorGen: 9, maxMatches: 50 }),
      expect.any(AbortSignal)
    )
  })

  it('(d) marks width-mismatched matches approximate (nearest-row-boundary remap)', async () => {
    const searchRemote: RemoteSearchCall = async () =>
      result({
        matches: [{ hostRow: 110, col: 0, len: 6, line: 'needle' }],
        total: 1,
        anchorHostCols: 132 // snapshot serialized at 132; client is 80
      })
    const adapter = createRemotePaneSearchAdapter({
      discoverRemotePanes: () => [remotePane({ clientCols: 80 })],
      searchRemote
    })
    const [batch] = await collect(adapter)
    expect(batch.matches[0]).toMatchObject({ absRow: 10, approximate: true })
  })

  it('(d) stable-rows-across-host-resize: uses the snapshot anchor width, not the live hostCols', async () => {
    // The host has resized its LIVE grid to 132 (hostCols: 132) but the snapshot
    // the client replayed was serialized at 80 (anchorHostCols: 80). The match
    // still comes back at the same stable host row 140 — it must remap to the same
    // exact client row (not approximate), proving the live resize can't move it.
    const searchRemote: RemoteSearchCall = async () =>
      result({
        matches: [{ hostRow: 140, col: 0, len: 6, line: 'needle' }],
        total: 1,
        hostCols: 132, // host LIVE width after resize
        anchorHostCols: 80 // SNAPSHOT width (what the client replayed at)
      })
    const adapter = createRemotePaneSearchAdapter({
      discoverRemotePanes: () => [remotePane({ clientCols: 80 })],
      searchRemote
    })
    const [batch] = await collect(adapter)
    // Exact jump at client row 40 — the live resize to 132 is irrelevant.
    expect(batch.matches[0]).toEqual({ absRow: 40, col: 0, len: 6, snippet: 'needle' })
    expect(batch.matches[0].approximate).toBeUndefined()
  })

  it('carries out-of-window / anchor-mismatch matches as inline-only (incomplete), never a wrong jump', async () => {
    const searchRemote: RemoteSearchCall = async () =>
      result({
        matches: [
          { hostRow: 110, col: 0, len: 6, line: 'in window' }, // client 10
          { hostRow: 50, col: 0, len: 6, line: 'deep history' } // older than replay → inline
        ],
        total: 2
      })
    const adapter = createRemotePaneSearchAdapter({
      discoverRemotePanes: () => [remotePane()],
      searchRemote
    })
    const [batch] = await collect(adapter)
    expect(batch.matches.map((m) => m.absRow)).toEqual([10]) // only the jumpable one
    expect(batch.total).toBe(2) // honest count
    expect(batch.incomplete).toBe(true) // the deep match needs inline expansion
  })

  it('degrades an anchor-mismatch (client replayed a different snapshot) to inline-only', async () => {
    const searchRemote: RemoteSearchCall = async () =>
      result({
        matches: [{ hostRow: 110, col: 0, len: 6, line: 'x' }],
        total: 1,
        anchorGen: 6 // response anchor gen != the replayed gen (5)
      })
    const adapter = createRemotePaneSearchAdapter({
      discoverRemotePanes: () => [remotePane()],
      searchRemote
    })
    const [batch] = await collect(adapter)
    expect(batch.matches).toEqual([])
    expect(batch.incomplete).toBe(true)
  })

  it('skips an unavailable host (old host / no model) — source absent, never an error', async () => {
    const searchRemote: RemoteSearchCall = async () => null
    const adapter = createRemotePaneSearchAdapter({
      discoverRemotePanes: () => [remotePane()],
      searchRemote
    })
    expect(await collect(adapter)).toEqual([])
  })

  it('a throwing transport does not fail the fan-out (other panes still answer)', async () => {
    const panes = [remotePane({ paneRef: { tabId: 't1', leafId: 'a', paneKey: 't1:a', worktreeId: null, title: null } }), remotePane({ sessionId: 's2', paneRef: { tabId: 't2', leafId: 'b', paneKey: 't2:b', worktreeId: null, title: null } })]
    const searchRemote: RemoteSearchCall = async (pane) => {
      if (pane.paneRef.paneKey === 't1:a') {
        throw new Error('channel died')
      }
      return result({ matches: [{ hostRow: 110, col: 0, len: 6, line: 'ok' }], total: 1 })
    }
    const adapter = createRemotePaneSearchAdapter({ discoverRemotePanes: () => panes, searchRemote })
    const batches = await collect(adapter)
    expect(batches).toHaveLength(1)
    expect(batches[0].paneRef?.paneKey).toBe('t2:b')
  })

  it('cancel(gen) aborts the in-flight host requests', async () => {
    const signals: AbortSignal[] = []
    const searchRemote: RemoteSearchCall = (_pane, _req, signal) =>
      new Promise((resolve) => {
        signals.push(signal)
        signal.addEventListener('abort', () => resolve(null))
      })
    const adapter = createRemotePaneSearchAdapter({
      discoverRemotePanes: () => [remotePane()],
      searchRemote
    })
    const done = adapter.query('needle', { caseSensitive: false, isRegex: false }, 3, 50, () => {})
    adapter.cancel(3)
    await done
    expect(signals[0]?.aborted).toBe(true)
  })
})
