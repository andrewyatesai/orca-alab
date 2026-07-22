// P1 worker side: a find rides the id-correlated query channel. The worker terminal
// answers `{count, activeIndex}` (decoding the flag bits), the P1.1 budgeted path runs
// the find in message-loop-yielding slices (a newer find observed between slices
// cancels the old run — cursor dropped, partial matches never surface), and the pane
// dispatch skips a queued find once a NEWER one has arrived.

import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWorkerTerminal, type WorkerTerminal } from './aterm-worker-terminal'
import { dispatchPaneCommand, type PaneRuntime } from './aterm-worker-pane-dispatch'
import {
  answerSearchFindQuery,
  createWorkerSearch,
  SEARCH_FIND_FLAG_CASE_SENSITIVE,
  SEARCH_FIND_FLAG_REGEX
} from './aterm-worker-search'
import type { EngineBudgetedSearchStep, EngineHandle } from './aterm-worker-engine-build'
import type { WorkerFrameScheduler } from './aterm-worker-frame-scheduler'

function makeSearchHandle(): { handle: EngineHandle; search: ReturnType<typeof vi.fn> } {
  const engine = {
    display_offset: 0,
    search_display_origin: 0,
    cell_width: 8,
    cell_height: 16,
    scroll_search_line_into_view: () => undefined
  }
  const search = vi.fn(() => new Uint32Array([3, 0, 4, 9, 2, 4]))
  const handle = {
    kind: 'cpu',
    engine: engine as unknown as EngineHandle['engine'],
    memory: { buffer: new ArrayBuffer(0) } as unknown as WebAssembly.Memory,
    process: () => undefined,
    render: () => undefined,
    framebuffer: () => ({ width: 0, height: 0 }),
    search,
    dispose: () => undefined
  } as EngineHandle
  return { handle, search }
}

describe('worker terminal searchFind query', () => {
  it('runs the find, decodes the flag bits, and answers the post-find state as JSON', () => {
    const { handle, search } = makeSearchHandle()
    const term = createWorkerTerminal(handle)

    const respond = vi.fn()
    term.searchFindQuery(
      SEARCH_FIND_FLAG_CASE_SENSITIVE | SEARCH_FIND_FLAG_REGEX,
      'needle',
      1,
      undefined,
      respond
    )

    expect(search).toHaveBeenCalledWith('needle', true, true)
    // One-shot fallback handle (no budgeted API) → the reply is synchronous.
    // Two matches; find selects the LAST (closest to the live bottom), 1-based.
    expect(respond).toHaveBeenCalledTimes(1)
    expect(JSON.parse(respond.mock.calls[0][0] as string)).toEqual({ count: 2, activeIndex: 2 })
  })

  it('treats absent flags/text as a plain case-insensitive literal find', () => {
    const { handle, search } = makeSearchHandle()
    const term = createWorkerTerminal(handle)
    term.searchFindQuery(undefined, 'abc', 1, undefined, () => undefined)
    expect(search).toHaveBeenCalledWith('abc', false, false)
  })
})

/** A budgeted engine step with test-relevant fields overridden. */
function step(over: Partial<EngineBudgetedSearchStep>): EngineBudgetedSearchStep {
  return {
    matches: new Uint32Array(0),
    complete: false,
    cursor: undefined,
    incompleteIndex: false,
    rowsFed: 0,
    totalRows: 0,
    ...over
  }
}

function makeBudgetedHandle(steps: EngineBudgetedSearchStep[]): {
  handle: EngineHandle
  searchBudgeted: ReturnType<typeof vi.fn>
  searchBudgetedCancel: ReturnType<typeof vi.fn>
  scrollIntoView: ReturnType<typeof vi.fn>
} {
  const scrollIntoView = vi.fn()
  const engine = {
    display_offset: 0,
    search_display_origin: 0,
    cell_width: 8,
    cell_height: 16,
    scroll_search_line_into_view: scrollIntoView
  }
  let i = 0
  const searchBudgeted = vi.fn(() => steps[Math.min(i++, steps.length - 1)])
  const searchBudgetedCancel = vi.fn()
  const handle = {
    kind: 'cpu',
    engine: engine as unknown as EngineHandle['engine'],
    memory: { buffer: new ArrayBuffer(0) } as unknown as WebAssembly.Memory,
    process: () => undefined,
    render: () => undefined,
    framebuffer: () => ({ width: 0, height: 0 }),
    search: vi.fn(() => new Uint32Array(0)),
    searchBudgeted,
    searchBudgetedCancel,
    dispose: () => undefined
  } as EngineHandle
  return { handle, searchBudgeted, searchBudgetedCancel, scrollIntoView }
}

describe('budgeted sliced find (P1.1)', () => {
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('cancels between slices when a newer find id is observed — cursor dropped, partial matches never surface', () => {
    vi.useFakeTimers()
    const { handle, searchBudgeted, searchBudgetedCancel } = makeBudgetedHandle([
      step({ matches: new Uint32Array([5, 0, 3]), cursor: 11, rowsFed: 4096, totalRows: 50000 })
    ])
    const search = createWorkerSearch(handle, () => 24)
    const respond = vi.fn()
    let latestFindId = 1
    answerSearchFindQuery(search, 0, 'needle', 1, () => 1 < latestFindId, respond)

    // First slice ran synchronously (incomplete), the run yielded to the message loop.
    expect(searchBudgeted).toHaveBeenCalledTimes(1)
    expect(respond).not.toHaveBeenCalled()

    latestFindId = 2 // a newer find ARRIVES between slices
    vi.runAllTimers()

    expect(searchBudgeted).toHaveBeenCalledTimes(1) // cursor dropped — never resumed
    expect(searchBudgetedCancel).toHaveBeenCalled() // engine's partial index freed
    expect(respond).toHaveBeenCalledTimes(1)
    expect(respond).toHaveBeenCalledWith(null) // settles like the supersede skip
    // The cancelled run's partial matches never surfaced.
    expect(search.count()).toBe(0)
    expect(search.generation()).toBe(0)
  })

  it('a newer find superseding mid-run settles the old query null and completes fresh (no stale cursor)', () => {
    vi.useFakeTimers()
    const { handle, searchBudgeted, searchBudgetedCancel } = makeBudgetedHandle([
      step({ matches: new Uint32Array([5, 0, 2]), cursor: 5, rowsFed: 4096, totalRows: 50000 }),
      step({
        matches: new Uint32Array([9, 2, 3]),
        complete: true,
        rowsFed: 50000,
        totalRows: 50000
      })
    ])
    const search = createWorkerSearch(handle, () => 24)
    const respond1 = vi.fn()
    const respond2 = vi.fn()
    answerSearchFindQuery(search, 0, 'aa', 1, undefined, respond1)
    expect(respond1).not.toHaveBeenCalled() // in flight after slice 1

    answerSearchFindQuery(search, 0, 'aab', 2, undefined, respond2)

    expect(respond1).toHaveBeenCalledWith(null) // old run settled, results dropped
    expect(searchBudgetedCancel).toHaveBeenCalled()
    // The new find started clean — same engine entry, no stale cursor.
    expect(searchBudgeted).toHaveBeenCalledTimes(2)
    expect(searchBudgeted.mock.calls[1].slice(0, 4)).toEqual(['aab', false, false, undefined])
    expect(JSON.parse(respond2.mock.calls[0][0] as string)).toEqual({ count: 1, activeIndex: 1 })
    expect(search.generation()).toBe(2)
    vi.runAllTimers() // the old run's armed slice timer was cleared — nothing fires
    expect(searchBudgeted).toHaveBeenCalledTimes(2)
    expect(respond1).toHaveBeenCalledTimes(1)
    expect(respond2).toHaveBeenCalledTimes(1)
  })

  it('resumes across slices via the cursor and answers only on completion', () => {
    vi.useFakeTimers()
    const { handle, searchBudgeted, scrollIntoView } = makeBudgetedHandle([
      step({ matches: new Uint32Array([3, 0, 4]), cursor: 7, rowsFed: 4096, totalRows: 8000 }),
      step({
        matches: new Uint32Array([3, 0, 4, 9, 2, 4]),
        complete: true,
        rowsFed: 8000,
        totalRows: 8000
      })
    ])
    const search = createWorkerSearch(handle, () => 24)
    const respond = vi.fn()
    answerSearchFindQuery(search, 0, 'ab', 3, () => false, respond)
    expect(respond).not.toHaveBeenCalled()

    vi.runAllTimers()

    expect(searchBudgeted).toHaveBeenCalledTimes(2)
    expect(searchBudgeted.mock.calls[1].slice(0, 4)).toEqual(['ab', false, false, 7]) // resumed
    expect(JSON.parse(respond.mock.calls[0][0] as string)).toEqual({ count: 2, activeIndex: 2 })
    expect(search.generation()).toBe(3)
    expect(scrollIntoView).toHaveBeenCalledWith(9) // LAST match scrolled into view
  })

  it('settles with the scanned prefix flagged stale after repeated engine restarts — every call slice-budgeted', () => {
    vi.useFakeTimers()
    // rowsFed DROPS on calls 2-4 (content changed between slices; the engine
    // restarted from row zero) — the third restart settles the run with the
    // step's partial matches instead of escalating to an unbounded call.
    const { handle, searchBudgeted } = makeBudgetedHandle([
      step({ cursor: 5, rowsFed: 4096, totalRows: 50000 }),
      step({ cursor: 6, rowsFed: 2000, totalRows: 50000 }),
      step({ cursor: 7, rowsFed: 1500, totalRows: 50000 }),
      step({ matches: new Uint32Array([3, 0, 4]), cursor: 8, rowsFed: 1000, totalRows: 50000 })
    ])
    const search = createWorkerSearch(handle, () => 24)
    const respond = vi.fn()
    // Make each mocked slice read as ~40ms of engine work so the settled cost
    // (~160ms) exceeds the refresh tick — the cost gate then KEEPS the partial
    // results stale instead of eagerly re-indexing on the reply read.
    let clock = 0
    vi.spyOn(performance, 'now').mockImplementation(() => (clock += 40))
    answerSearchFindQuery(search, 0, 'ab', 5, () => false, respond)

    // Advance only the 0ms slice timers — the trailing refresh (>=100ms) stays
    // armed so the settled-stale state is observable.
    vi.advanceTimersByTime(50)

    // Settled on the 4th call — no 5th call, and no call ever exceeded the
    // adaptive slice bound (the unbounded-budget path no longer exists).
    expect(searchBudgeted).toHaveBeenCalledTimes(4)
    for (const call of searchBudgeted.mock.calls) {
      expect(call[4]).toBeLessThanOrEqual(262144)
    }
    expect(handle.search).not.toHaveBeenCalled()
    // The prefix's matches surface immediately but flagged STALE, with the
    // trailing refresh armed to deliver the final answer.
    expect(JSON.parse(respond.mock.calls[0][0] as string)).toEqual({ count: 1, activeIndex: 1 })
    expect(search.resultsStale()).toBe(true)
    expect(search.generation()).toBe(5)

    // The armed trailing refresh then delivers the FINAL answer through the
    // cost-gated re-index path and clears the stale flag.
    vi.runAllTimers()
    expect(handle.search).toHaveBeenCalledTimes(1)
    expect(search.resultsStale()).toBe(false)
  })

  it('a newer find id observed between slices cancels a restarting run before it settles', () => {
    vi.useFakeTimers()
    const { handle, searchBudgeted, searchBudgetedCancel } = makeBudgetedHandle([
      step({ cursor: 5, rowsFed: 4096, totalRows: 50000 }),
      step({ cursor: 6, rowsFed: 2000, totalRows: 50000 }),
      step({ cursor: 7, rowsFed: 1500, totalRows: 50000 }),
      step({ matches: new Uint32Array([3, 0, 4]), cursor: 8, rowsFed: 1000, totalRows: 50000 })
    ])
    const search = createWorkerSearch(handle, () => 24)
    const respond = vi.fn()
    let latestFindId = 6
    answerSearchFindQuery(search, 0, 'ab', 6, () => 6 < latestFindId, respond)

    // Run three slices (two restarts observed), then a newer find arrives
    // BEFORE the next slice executes.
    for (let i = 0; i < 2; i++) {
      vi.advanceTimersToNextTimer()
    }
    expect(searchBudgeted).toHaveBeenCalledTimes(3)
    latestFindId = 7
    vi.runAllTimers()

    expect(searchBudgeted).toHaveBeenCalledTimes(3) // the 4th slice never ran
    expect(searchBudgetedCancel).toHaveBeenCalled()
    expect(handle.search).not.toHaveBeenCalled()
    expect(respond).toHaveBeenCalledWith(null)
    expect(search.count()).toBe(0)
  })
})

function makePane(term: WorkerTerminal | null): {
  pane: PaneRuntime
  posted: { id: number; value: unknown }[]
  schedule: ReturnType<typeof vi.fn>
} {
  const posted: { id: number; value: unknown }[] = []
  const schedule = vi.fn()
  const pane = {
    paneId: 1,
    term,
    engineSetters: null,
    engine: null,
    engineKind: null,
    engineMemory: null,
    storedInit: null,
    canvas: null,
    fellBackToCpu: false,
    disposed: false,
    latestSearchFindQueryId: 0,
    chrome: { pad: 0, head: 0 },
    frameScheduler: { schedule } as unknown as WorkerFrameScheduler,
    serializeCache: { schedule: () => undefined, dispose: () => undefined },
    post: (event: unknown) => {
      const e = event as { type: string; id: number; value: unknown }
      if (e.type === 'queryResult') {
        posted.push({ id: e.id, value: e.value })
      }
    }
  } as PaneRuntime
  return { pane, posted, schedule }
}

describe('pane dispatch searchFind supersede skip', () => {
  it('skips a find whose id is older than the newest ARRIVED find (no engine work)', () => {
    const searchFindQuery = vi.fn(
      (
        _arg: number | undefined,
        _text: string | undefined,
        _id: number,
        _isCancelled: (() => boolean) | undefined,
        respond: (value: string | null) => void
      ) => respond('{"count":1,"activeIndex":1}')
    )
    const term = { searchFindQuery } as unknown as WorkerTerminal
    const { pane, posted, schedule } = makePane(term)

    // Both finds arrived (entry records the newest id) before the FIRST executes —
    // the flood-backlog case where the old find sat queued behind bulk process.
    pane.latestSearchFindQueryId = 2
    dispatchPaneCommand(pane, { type: 'query', id: 1, kind: 'searchFind', arg: 0, text: 'a' })

    expect(searchFindQuery).not.toHaveBeenCalled() // superseded → the engine search never runs
    expect(schedule).not.toHaveBeenCalled()
    expect(posted).toEqual([{ id: 1, value: null }])

    // The newest find still executes, repaints, and answers.
    dispatchPaneCommand(pane, { type: 'query', id: 2, kind: 'searchFind', arg: 0, text: 'ab' })
    // The query id doubles as the request generation the STATE echoes.
    expect(searchFindQuery).toHaveBeenCalledWith(
      0,
      'ab',
      2,
      expect.any(Function),
      expect.any(Function)
    )
    expect(schedule).toHaveBeenCalledTimes(1)
    expect(posted[1]).toEqual({ id: 2, value: '{"count":1,"activeIndex":1}' })
  })

  it('does not schedule a repaint when the find settles null (cancelled mid-slices)', () => {
    const searchFindQuery = vi.fn(
      (
        _arg: number | undefined,
        _text: string | undefined,
        _id: number,
        _isCancelled: (() => boolean) | undefined,
        respond: (value: string | null) => void
      ) => respond(null)
    )
    const term = { searchFindQuery } as unknown as WorkerTerminal
    const { pane, posted, schedule } = makePane(term)
    pane.latestSearchFindQueryId = 1
    dispatchPaneCommand(pane, { type: 'query', id: 1, kind: 'searchFind', arg: 0, text: 'a' })
    expect(schedule).not.toHaveBeenCalled() // no results adopted → nothing to repaint
    expect(posted).toEqual([{ id: 1, value: null }])
  })

  it('answers null (not a crash) when the engine is still building', () => {
    const { pane, posted } = makePane(null)
    pane.latestSearchFindQueryId = 1
    dispatchPaneCommand(pane, { type: 'query', id: 1, kind: 'searchFind', arg: 0, text: 'a' })
    expect(posted).toEqual([{ id: 1, value: null }])
  })
})
