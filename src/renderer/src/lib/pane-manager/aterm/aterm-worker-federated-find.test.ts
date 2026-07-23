import { describe, expect, it, vi } from 'vitest'
import {
  createWorkerFederatedFind,
  FEDERATED_INDEX_BYTES_PER_LINE,
  FEDERATED_WORKER_INDEX_BUDGET_BYTES,
  type FederatedFindPaneSource
} from './aterm-worker-federated-find'
import { createWorkerSearch } from './aterm-worker-search'
import type { AtermWorkerFederatedEvent } from './aterm-worker-federated-protocol'
import type { EngineBudgetedSearchStep } from './aterm-engine-budgeted-search'

function completeStep(matches: number[]): EngineBudgetedSearchStep {
  return {
    matches: new Uint32Array(matches),
    complete: true,
    cursor: undefined,
    reset: true,
    incompleteIndex: false,
    rowsFed: 10,
    totalRows: 10
  }
}

function makePane(opts?: {
  matches?: number[]
  lines?: number
  onScan?: () => void
}): FederatedFindPaneSource & {
  evictBudgeted: ReturnType<typeof vi.fn>
  evictWarm: ReturnType<typeof vi.fn>
} {
  const evictBudgeted = vi.fn()
  const evictWarm = vi.fn()
  return {
    engine: {
      searchBudgeted: vi.fn(() => {
        opts?.onScan?.()
        return completeStep(opts?.matches ?? [4, 0, 3])
      }),
      searchBudgetedCancel: evictBudgeted
    },
    baseY: () => opts?.lines ?? 100,
    rows: () => 24,
    evictBudgetedState: evictBudgeted,
    evictWarmIndex: evictWarm,
    evictBudgeted,
    evictWarm
  }
}

async function settle(): Promise<void> {
  // Drain the setTimeout(0) slice/pane chain.
  for (let i = 0; i < 50; i++) {
    await new Promise((r) => setTimeout(r, 0))
  }
}

describe('createWorkerFederatedFind', () => {
  it('walks panes SERIALLY in the supplied order and streams one batch per pane', async () => {
    const order: number[] = []
    const panes = new Map<number, ReturnType<typeof makePane>>([
      [1, makePane({ matches: [10, 0, 3], onScan: () => order.push(1) })],
      [2, makePane({ matches: [20, 0, 3], onScan: () => order.push(2) })],
      [3, makePane({ matches: [30, 0, 3], onScan: () => order.push(3) })]
    ])
    const events: AtermWorkerFederatedEvent[] = []
    const runner = createWorkerFederatedFind({
      resolvePane: (id) => panes.get(id) ?? null,
      post: (event) => events.push(event)
    })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [
        { paneId: 2, visible: true },
        { paneId: 3, visible: false },
        { paneId: 1, visible: false }
      ]
    })
    await settle()
    expect(order).toEqual([2, 3, 1])
    const batches = events.filter(
      (e): e is Extract<AtermWorkerFederatedEvent, { type: 'federatedBatch' }> =>
        e.type === 'federatedBatch'
    )
    expect(batches.map((b) => b.paneId)).toEqual([2, 3, 1])
    const done = events.at(-1)
    expect(done).toMatchObject({ type: 'federatedDone', gen: 1, cancelled: false })
  })

  it('evicts non-visible panes immediately after their scan; visible panes keep the index', async () => {
    const visiblePane = makePane()
    const hiddenPane = makePane()
    const panes = new Map([
      [1, visiblePane],
      [2, hiddenPane]
    ])
    const runner = createWorkerFederatedFind({
      resolvePane: (id) => panes.get(id) ?? null,
      post: () => undefined
    })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [
        { paneId: 1, visible: true },
        { paneId: 2, visible: false }
      ]
    })
    await settle()
    expect(visiblePane.evictBudgeted).not.toHaveBeenCalled()
    expect(visiblePane.evictWarm).not.toHaveBeenCalled()
    expect(hiddenPane.evictBudgeted).toHaveBeenCalled()
    expect(hiddenPane.evictWarm).toHaveBeenCalled()
  })

  it('refuses to index a pane whose estimate breaches the §4 budget (honest empty batch)', async () => {
    const overBudgetLines =
      Math.ceil(FEDERATED_WORKER_INDEX_BUDGET_BYTES / FEDERATED_INDEX_BYTES_PER_LINE) + 1
    const hugePane = makePane({ lines: overBudgetLines })
    const events: AtermWorkerFederatedEvent[] = []
    const runner = createWorkerFederatedFind({
      resolvePane: () => hugePane,
      post: (event) => events.push(event)
    })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [{ paneId: 1, visible: false }]
    })
    await settle()
    expect(hugePane.engine.searchBudgeted).not.toHaveBeenCalled()
    const batch = events.find((e) => e.type === 'federatedBatch')
    expect(batch).toMatchObject({
      matches: [],
      total: 0,
      incomplete: true,
      degraded: 'over-budget'
    })
  })

  it('degrades an over-budget pane to the UNINDEXED linear scan when a reader is available (never silent)', async () => {
    const overBudgetLines =
      Math.ceil(FEDERATED_WORKER_INDEX_BUDGET_BYTES / FEDERATED_INDEX_BYTES_PER_LINE) + 1
    const pane: FederatedFindPaneSource = {
      engine: { searchBudgeted: vi.fn() },
      baseY: () => overBudgetLines,
      rows: () => 24,
      linearScanReader: () => ({
        oldestAbsRow: 0,
        rowCount: 3,
        read: (first, count) =>
          ['alpha needle', 'beta', 'gamma needle'].slice(first, first + count)
      })
    }
    const events: AtermWorkerFederatedEvent[] = []
    const runner = createWorkerFederatedFind({ resolvePane: () => pane, post: (e) => events.push(e) })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'needle',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [{ paneId: 1, visible: false }]
    })
    await settle()
    // The index path was NOT taken (no posting index built for the over-budget pane).
    expect(pane.engine.searchBudgeted).not.toHaveBeenCalled()
    const batch = events.find((e) => e.type === 'federatedBatch') as Extract<
      AtermWorkerFederatedEvent,
      { type: 'federatedBatch' }
    >
    expect(batch.degraded).toBe('linear-scan')
    // Real matches (newest-first, raw-text snippets) — not a silent no-results.
    expect(batch.matches.map((m) => m.absRow)).toEqual([2, 0])
    expect(batch.matches[0].snippet).toBe('gamma needle')
  })

  it('counts VISIBLE panes against the resident budget so later panes get refused', async () => {
    // Each visible pane estimates just over half the budget: the second one
    // (also visible) no longer fits and must be refused, not indexed.
    const halfBudgetLines = Math.ceil(
      FEDERATED_WORKER_INDEX_BUDGET_BYTES / FEDERATED_INDEX_BYTES_PER_LINE / 2 + 1
    )
    const first = makePane({ lines: halfBudgetLines })
    const second = makePane({ lines: halfBudgetLines })
    const panes = new Map([
      [1, first],
      [2, second]
    ])
    const events: AtermWorkerFederatedEvent[] = []
    const runner = createWorkerFederatedFind({
      resolvePane: (id) => panes.get(id) ?? null,
      post: (event) => events.push(event)
    })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [
        { paneId: 1, visible: true },
        { paneId: 2, visible: true }
      ]
    })
    await settle()
    expect(first.engine.searchBudgeted).toHaveBeenCalled()
    expect(second.engine.searchBudgeted).not.toHaveBeenCalled()
    const batches = events.filter((e) => e.type === 'federatedBatch')
    expect(batches[1]).toMatchObject({ paneId: 2, degraded: 'over-budget' })
  })

  it('a federatedCancel for the live gen stops the walk (done reports cancelled)', async () => {
    const events: AtermWorkerFederatedEvent[] = []
    const panes = new Map([
      [1, makePane()],
      [2, makePane()]
    ])
    const runner = createWorkerFederatedFind({
      resolvePane: (id) => panes.get(id) ?? null,
      post: (event) => events.push(event)
    })
    runner.dispatch({
      type: 'federatedFind',
      gen: 7,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [
        { paneId: 1, visible: false },
        { paneId: 2, visible: false }
      ]
    })
    // Cancel synchronously — before the first setTimeout slice runs.
    runner.dispatch({ type: 'federatedCancel', gen: 7 })
    await settle()
    const done = events.find((e) => e.type === 'federatedDone')
    expect(done).toMatchObject({ gen: 7, cancelled: true })
    // Pane 2 never scanned (serial walk stopped).
    expect(panes.get(2)!.engine.searchBudgeted).not.toHaveBeenCalled()
  })

  it('a new federatedFind supersedes the in-flight run', async () => {
    const events: AtermWorkerFederatedEvent[] = []
    const pane = makePane()
    const runner = createWorkerFederatedFind({
      resolvePane: () => pane,
      post: (event) => events.push(event)
    })
    const find = (gen: number): void =>
      runner.dispatch({
        type: 'federatedFind',
        gen,
        query: 'foo',
        caseSensitive: false,
        isRegex: false,
        maxPerPane: 50,
        panes: [{ paneId: 1, visible: false }]
      })
    find(1)
    find(2)
    await settle()
    const dones = events.filter((e) => e.type === 'federatedDone')
    expect(dones).toContainEqual({ type: 'federatedDone', gen: 1, cancelled: true })
    expect(dones).toContainEqual({ type: 'federatedDone', gen: 2, cancelled: false })
  })

  it('skips a pane that disposed between snapshot and walk without failing the run', async () => {
    const events: AtermWorkerFederatedEvent[] = []
    const alive = makePane()
    const runner = createWorkerFederatedFind({
      resolvePane: (id) => (id === 2 ? alive : null),
      post: (event) => events.push(event)
    })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [
        { paneId: 1, visible: false },
        { paneId: 2, visible: false }
      ]
    })
    await settle()
    const batches = events.filter((e) => e.type === 'federatedBatch')
    expect(batches).toHaveLength(1)
    expect(batches[0]).toMatchObject({ paneId: 2 })
    expect(events.at(-1)).toMatchObject({ type: 'federatedDone', cancelled: false })
  })

  it('enriches match snippets when the pane exposes search_summary (E-1 consumption)', async () => {
    const readSnippets = vi.fn(
      () =>
        new Map([
          [30, 'x[foo]y'],
          [4, '[foo]']
        ])
    )
    const pane: FederatedFindPaneSource = {
      engine: { searchBudgeted: vi.fn(() => completeStep([30, 0, 3, 4, 0, 3])) },
      baseY: () => 100,
      rows: () => 24,
      readSnippets
    }
    const events: AtermWorkerFederatedEvent[] = []
    const runner = createWorkerFederatedFind({ resolvePane: () => pane, post: (e) => events.push(e) })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [{ paneId: 1, visible: true }]
    })
    await settle()
    expect(readSnippets).toHaveBeenCalledExactlyOnceWith('foo', false, false, 50)
    const batch = events.find((e) => e.type === 'federatedBatch') as Extract<
      AtermWorkerFederatedEvent,
      { type: 'federatedBatch' }
    >
    // Newest-first, snippets attached by absRow.
    expect(batch.matches).toEqual([
      { absRow: 30, col: 0, len: 3, snippet: 'x[foo]y' },
      { absRow: 4, col: 0, len: 3, snippet: '[foo]' }
    ])
  })

  it('leaves snippets null on pins WITHOUT search_summary (count-only degradation)', async () => {
    const pane: FederatedFindPaneSource = {
      engine: { searchBudgeted: vi.fn(() => completeStep([7, 0, 3])) },
      baseY: () => 100,
      rows: () => 24
      // no readSnippets — pre-E-1 pin
    }
    const events: AtermWorkerFederatedEvent[] = []
    const runner = createWorkerFederatedFind({ resolvePane: () => pane, post: (e) => events.push(e) })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [{ paneId: 1, visible: true }]
    })
    await settle()
    const batch = events.find((e) => e.type === 'federatedBatch') as Extract<
      AtermWorkerFederatedEvent,
      { type: 'federatedBatch' }
    >
    expect(batch.matches).toEqual([{ absRow: 7, col: 0, len: 3, snippet: null }])
  })

  // §6 worker-memory instrumentation (admission control): N synthetic cold panes
  // must never leave more than K resident indexes at once. A pane becomes
  // "resident" when its budgeted scan first builds an index and stops being
  // resident when its state is evicted; the serial + immediate-eviction rule
  // holds the peak at 1 for non-visible panes and the design ceiling K=2 overall.
  it('§6: N cold panes never exceed K resident indexes (peak-resident instrumentation)', async () => {
    const FEDERATED_ADMISSION_K = 2
    let resident = 0
    let peakResident = 0
    const makeInstrumentedPane = (): FederatedFindPaneSource => {
      let isResident = false
      const free = (): void => {
        if (isResident) {
          isResident = false
          resident--
        }
      }
      return {
        engine: {
          searchBudgeted: vi.fn(() => {
            if (!isResident) {
              isResident = true
              resident++
              peakResident = Math.max(peakResident, resident)
            }
            return completeStep([4, 0, 3])
          }),
          searchBudgetedCancel: free
        },
        baseY: () => 100,
        rows: () => 24,
        evictBudgetedState: free,
        evictWarmIndex: () => undefined
      }
    }
    const N = 12
    const panes = new Map<number, FederatedFindPaneSource>()
    for (let i = 1; i <= N; i++) {
      panes.set(i, makeInstrumentedPane())
    }
    const runner = createWorkerFederatedFind({
      resolvePane: (id) => panes.get(id) ?? null,
      post: () => undefined
    })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: Array.from({ length: N }, (_, i) => ({ paneId: i + 1, visible: false }))
    })
    await settle()
    expect(peakResident).toBeLessThanOrEqual(FEDERATED_ADMISSION_K)
    // Serial walk + immediate eviction: exactly one resident at a time here.
    expect(peakResident).toBe(1)
    // Every non-visible pane's index was released — nothing left resident.
    expect(resident).toBe(0)
  })

  it("never perturbs a pane's active find-bar state (count/activeIndex unchanged)", async () => {
    // A REAL WorkerSearch holding results on the same engine surface the
    // federated scan drives: the fan-out must leave its published state intact.
    const engineStub = {
      scroll_search_line_into_view: vi.fn(),
      search_display_origin: 0,
      display_offset: 0,
      base_y: 100,
      cell_width: 8,
      cell_height: 16
    }
    // No searchBudgeted → WorkerSearch takes the legacy one-shot path.
    const handle = {
      engine: engineStub,
      search: vi.fn(() => new Uint32Array([3, 0, 4, 9, 0, 4]))
    } as unknown as Parameters<typeof createWorkerSearch>[0]
    const paneFind = createWorkerSearch(handle, () => 24)
    paneFind.find('bar', false, false)
    expect(paneFind.count()).toBe(2)
    const activeBefore = paneFind.activeIndex()

    const pane: FederatedFindPaneSource = {
      engine: {
        searchBudgeted: vi.fn(() => completeStep([50, 0, 3]))
      },
      baseY: () => 100,
      rows: () => 24
    }
    const runner = createWorkerFederatedFind({
      resolvePane: () => pane,
      post: () => undefined
    })
    runner.dispatch({
      type: 'federatedFind',
      gen: 1,
      query: 'foo',
      caseSensitive: false,
      isRegex: false,
      maxPerPane: 50,
      panes: [{ paneId: 1, visible: true }]
    })
    await settle()
    expect(paneFind.count()).toBe(2)
    expect(paneFind.activeIndex()).toBe(activeBefore)
    // The federated run scrolled nothing (no scroll_search_line_into_view calls
    // beyond the pane find's own).
    expect(engineStub.scroll_search_line_into_view).toHaveBeenCalledTimes(1)
  })
})
