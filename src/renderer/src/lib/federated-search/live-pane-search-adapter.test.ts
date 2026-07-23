import { describe, expect, it, vi } from 'vitest'
import {
  createLivePaneSearchAdapter,
  orderLivePanes,
  type DiscoveredLivePane
} from './live-pane-search-adapter'
import type {
  AtermWorkerFederatedCommand,
  AtermWorkerFederatedEvent
} from '../pane-manager/aterm/aterm-worker-federated-protocol'
import {
  FEDERATED_INDEX_BYTES_PER_LINE,
  FEDERATED_WORKER_INDEX_BUDGET_BYTES
} from '../pane-manager/aterm/aterm-worker-federated-find'
import type { FederatedPaneBatch } from './federated-search-model'

function workerPane(
  paneKey: string,
  workerPaneId: number,
  opts?: Partial<Pick<DiscoveredLivePane, 'visible' | 'focused' | 'lastOutputAt' | 'sessionId'>>
): DiscoveredLivePane {
  const [tabId, leafId] = paneKey.split(':')
  return {
    paneRef: { tabId, leafId, paneKey, worktreeId: null, title: null },
    visible: opts?.visible ?? false,
    focused: opts?.focused ?? false,
    sessionId: opts?.sessionId ?? null,
    lastOutputAt: opts?.lastOutputAt ?? 0,
    target: { kind: 'worker', workerPaneId }
  }
}

function inProcessPane(
  paneKey: string,
  matches: number[],
  opts?: Partial<Pick<DiscoveredLivePane, 'visible' | 'focused'>> & {
    baseY?: number
    linearScanRows?: string[]
  }
): DiscoveredLivePane {
  const [tabId, leafId] = paneKey.split(':')
  return {
    paneRef: { tabId, leafId, paneKey, worktreeId: null, title: null },
    visible: opts?.visible ?? false,
    focused: opts?.focused ?? false,
    sessionId: null,
    lastOutputAt: 0,
    target: {
      kind: 'in-process',
      // E-6: only the budgeted cursor surface exists on the scan engine.
      engine: {
        searchBudgeted: () => ({
          matches: new Uint32Array(matches),
          complete: true,
          cursor: undefined,
          reset: true,
          incompleteIndex: false,
          rowsFed: 124,
          totalRows: 124
        })
      },
      baseY: () => opts?.baseY ?? 100,
      rows: () => 24,
      linearScanReader: opts?.linearScanRows
        ? () => ({
            oldestAbsRow: 0,
            rowCount: opts.linearScanRows!.length,
            read: (first, count) => opts.linearScanRows!.slice(first, first + count)
          })
        : undefined
    }
  }
}

describe('orderLivePanes', () => {
  it('orders focused → visible → recency of last output (§2.1 priority)', () => {
    const ordered = orderLivePanes([
      workerPane('t:old', 1, { lastOutputAt: 1 }),
      workerPane('t:focused', 2, { focused: true }),
      workerPane('t:recent', 3, { lastOutputAt: 100 }),
      workerPane('t:visible', 4, { visible: true })
    ])
    expect(ordered.map((p) => p.paneRef.paneKey)).toEqual([
      't:focused',
      't:visible',
      't:recent',
      't:old'
    ])
  })
})

describe('createLivePaneSearchAdapter (worker fan-out)', () => {
  it('posts ONE federatedFind for all worker panes, in priority order, and maps batches back', async () => {
    const posted: AtermWorkerFederatedCommand[] = []
    let workerHandler: ((e: AtermWorkerFederatedEvent) => void) | null = null
    const adapter = createLivePaneSearchAdapter({
      discoverPanes: () => [
        workerPane('t:bg', 11),
        workerPane('t:fg', 12, { focused: true, sessionId: 's-live' })
      ],
      postFederated: (cmd) => {
        posted.push(cmd)
        return true
      },
      subscribeFederated: (h) => {
        workerHandler = h
        return () => undefined
      },
      yieldIdle: (next) => next()
    })
    const batches: FederatedPaneBatch[] = []
    const run = adapter.query('foo', { caseSensitive: false, isRegex: false }, 1, 50, (b) =>
      batches.push(b)
    )
    expect(posted).toHaveLength(1)
    const find = posted[0]
    expect(find).toMatchObject({ type: 'federatedFind', gen: 1, query: 'foo' })
    if (find.type === 'federatedFind') {
      expect(find.panes.map((p) => p.paneId)).toEqual([12, 11])
      expect(find.panes[0].visible).toBe(true)
    }
    workerHandler!({
      type: 'federatedBatch',
      gen: 1,
      paneId: 12,
      matches: [{ absRow: 7, col: 0, len: 3, snippet: 'foo' }],
      total: 1,
      incomplete: false,
      degraded: 'none'
    })
    workerHandler!({ type: 'federatedDone', gen: 1, cancelled: false })
    await run
    expect(batches).toHaveLength(1)
    expect(batches[0]).toMatchObject({
      paneRef: { paneKey: 't:fg' },
      sessionId: 's-live',
      source: 'live',
      total: 1
    })
  })

  it('drops batches from a different generation', async () => {
    let workerHandler: ((e: AtermWorkerFederatedEvent) => void) | null = null
    const adapter = createLivePaneSearchAdapter({
      discoverPanes: () => [workerPane('t:a', 1)],
      postFederated: () => true,
      subscribeFederated: (h) => {
        workerHandler = h
        return () => undefined
      }
    })
    const batches: FederatedPaneBatch[] = []
    const run = adapter.query('foo', { caseSensitive: false, isRegex: false }, 2, 50, (b) =>
      batches.push(b)
    )
    workerHandler!({
      type: 'federatedBatch',
      gen: 1, // stale
      paneId: 1,
      matches: [{ absRow: 7, col: 0, len: 3, snippet: null }],
      total: 1,
      incomplete: false,
      degraded: 'none'
    })
    workerHandler!({ type: 'federatedDone', gen: 2, cancelled: false })
    await run
    expect(batches).toHaveLength(0)
  })

  it('resolves immediately when no live worker exists (post returns false)', async () => {
    const adapter = createLivePaneSearchAdapter({
      discoverPanes: () => [workerPane('t:a', 1)],
      postFederated: () => false,
      subscribeFederated: () => () => undefined
    })
    const batches: FederatedPaneBatch[] = []
    await adapter.query('foo', { caseSensitive: false, isRegex: false }, 1, 50, (b) =>
      batches.push(b)
    )
    expect(batches).toHaveLength(0)
  })

  it('cancel posts a federatedCancel and suppresses later emits for that gen', async () => {
    const posted: AtermWorkerFederatedCommand[] = []
    let workerHandler: ((e: AtermWorkerFederatedEvent) => void) | null = null
    const adapter = createLivePaneSearchAdapter({
      discoverPanes: () => [workerPane('t:a', 1)],
      postFederated: (cmd) => {
        posted.push(cmd)
        return true
      },
      subscribeFederated: (h) => {
        workerHandler = h
        return () => undefined
      }
    })
    const batches: FederatedPaneBatch[] = []
    const run = adapter.query('foo', { caseSensitive: false, isRegex: false }, 3, 50, (b) =>
      batches.push(b)
    )
    adapter.cancel(3)
    expect(posted.at(-1)).toEqual({ type: 'federatedCancel', gen: 3 })
    workerHandler!({
      type: 'federatedBatch',
      gen: 3,
      paneId: 1,
      matches: [{ absRow: 1, col: 0, len: 1, snippet: null }],
      total: 1,
      incomplete: false,
      degraded: 'none'
    })
    workerHandler!({ type: 'federatedDone', gen: 3, cancelled: true })
    await run
    expect(batches).toHaveLength(0)
  })
})

describe('createLivePaneSearchAdapter (in-process fallback)', () => {
  it('scans in-process panes on the main thread and emits their batches', async () => {
    const adapter = createLivePaneSearchAdapter({
      discoverPanes: () => [inProcessPane('t:ip', [5, 0, 3, 9, 1, 3], { visible: true })],
      postFederated: () => false,
      subscribeFederated: () => () => undefined,
      yieldIdle: (next) => next()
    })
    const batches: FederatedPaneBatch[] = []
    await adapter.query('foo', { caseSensitive: false, isRegex: false }, 1, 50, (b) =>
      batches.push(b)
    )
    expect(batches).toHaveLength(1)
    expect(batches[0].matches.map((m) => m.absRow)).toEqual([9, 5])
    expect(batches[0].source).toBe('live')
  })

  it('a hidden in-process pane is evicted after its scan', async () => {
    const cancel = vi.fn()
    const pane = inProcessPane('t:hidden', [1, 0, 1])
    if (pane.target.kind === 'in-process') {
      pane.target.engine.searchBudgetedCancel = cancel
    }
    const adapter = createLivePaneSearchAdapter({
      discoverPanes: () => [pane],
      postFederated: () => false,
      subscribeFederated: () => () => undefined,
      yieldIdle: (next) => next()
    })
    await adapter.query('foo', { caseSensitive: false, isRegex: false }, 1, 50, () => undefined)
    expect(cancel).toHaveBeenCalled()
  })

  it('admission is CUMULATIVE across visible in-process panes (§4 hard budget)', async () => {
    // Each pane alone fits the 256MB estimate budget; two visible panes keep
    // their warm indexes, so the third must be refused, not judged in isolation.
    const deepRows = Math.floor((100 * 1024 * 1024) / FEDERATED_INDEX_BYTES_PER_LINE)
    const panes = [
      inProcessPane('t:v1', [1, 0, 1], { visible: true, baseY: deepRows }),
      inProcessPane('t:v2', [2, 0, 1], { visible: true, baseY: deepRows }),
      inProcessPane('t:v3', [3, 0, 1], { visible: true, baseY: deepRows })
    ]
    const adapter = createLivePaneSearchAdapter({
      discoverPanes: () => panes,
      postFederated: () => false,
      subscribeFederated: () => () => undefined,
      yieldIdle: (next) => next()
    })
    const batches: FederatedPaneBatch[] = []
    await adapter.query('foo', { caseSensitive: false, isRegex: false }, 1, 50, (b) =>
      batches.push(b)
    )
    expect(batches).toHaveLength(3)
    expect(batches[0].degraded).toBe('none')
    expect(batches[1].degraded).toBe('none')
    // Third pane: cumulative retained estimate would breach the budget.
    expect(batches[2].degraded).toBe('over-budget')
    expect(batches[2].matches).toEqual([])
    expect(batches[2].incomplete).toBe(true)
  })

  it('§4: an over-budget in-process pane WITH a reader degrades to the linear scan (never silent)', async () => {
    const overBudget = Math.ceil(FEDERATED_WORKER_INDEX_BUDGET_BYTES / FEDERATED_INDEX_BYTES_PER_LINE) + 1
    const pane = inProcessPane('t:huge', [], {
      baseY: overBudget,
      linearScanRows: ['alpha needle', 'beta', 'gamma needle']
    })
    const adapter = createLivePaneSearchAdapter({
      discoverPanes: () => [pane],
      postFederated: () => false,
      subscribeFederated: () => () => undefined,
      yieldIdle: (next) => next()
    })
    const batches: FederatedPaneBatch[] = []
    await adapter.query('needle', { caseSensitive: false, isRegex: false }, 1, 50, (b) =>
      batches.push(b)
    )
    expect(batches).toHaveLength(1)
    expect(batches[0].degraded).toBe('linear-scan')
    expect(batches[0].matches.map((m) => m.absRow)).toEqual([2, 0])
    expect(batches[0].matches[0].snippet).toBe('gamma needle')
  })
})
