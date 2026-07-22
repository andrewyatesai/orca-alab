// P6 main side: search result VERSIONING must reach the UI — a re-index that changes
// match positions (or the cost-gate stale flag) with an identical count/active pair
// still has to fire onSearchStateChange, and the snapshot must expose the stale flag.

import { describe, expect, it, vi } from 'vitest'
import { createWorkerBackedTerm } from './aterm-worker-term'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'
import type { AtermWorkerPaneCommand, AtermWorkerState } from './aterm-render-worker-protocol'

function makeState(overrides: Partial<AtermWorkerState> = {}): AtermWorkerState {
  return {
    type: 'state',
    engine: 'cpu',
    wasmHeapBytes: 0,
    width: 0,
    height: 0,
    chromePadPx: 0,
    chromeHeadPx: 0,
    cols: 80,
    rows: 24,
    cellWidth: 8,
    cellHeight: 16,
    displayOffset: 0,
    displayOriginAbsolute: 0,
    cursorX: 0,
    cursorY: 0,
    cursorStyle: 1,
    baseY: 0,
    isAltScreen: false,
    bracketedPasteMode: false,
    isMouseTracking: false,
    mouseWantsMotion: false,
    mouseWantsAnyMotion: false,
    isFocusEventMode: false,
    isColorSchemeUpdatesMode: false,
    isAppCursorMode: false,
    isAlternateScroll: false,
    keyboardModeBits: 0,
    isReady: true,
    title: null,
    cursorColor: null,
    selectionRange: null,
    hoverLink: null,
    hoverCursor: '',
    searchCount: 0,
    searchActiveIndex: 0,
    searchActiveRect: null,
    searchResultsVersion: 0,
    searchResultsStale: false,
    searchResultsIncomplete: false,
    searchGeneration: 0,
    searchMarkers: { fractions: [], activeFraction: null },
    searchMatchRects: [],
    spillExportCapable: false,
    dirtyRows: [],
    predictOverlay: new Uint32Array(0),
    predictDeadlineMs: null,
    ...overrides
  }
}

function makeTerm(): {
  facade: AtermWorkerAsyncFacade
  applyState: (s: AtermWorkerState) => void
  posted: AtermWorkerPaneCommand[]
  resolveQuery: (id: number, value: string | number | boolean | null) => void
} {
  const posted: AtermWorkerPaneCommand[] = []
  const backed = createWorkerBackedTerm({
    post: (cmd) => posted.push(cmd),
    initial: makeState()
  })
  return {
    facade: backed.term as unknown as AtermWorkerAsyncFacade,
    applyState: backed.applyState,
    posted,
    resolveQuery: backed.resolveQuery
  }
}

describe('worker-backed term search state', () => {
  it('fires onSearchStateChange on a version bump even with identical count/active', () => {
    const { facade, applyState } = makeTerm()
    const handler = vi.fn()
    facade.onSearchStateChange(handler)

    applyState(makeState({ searchCount: 2, searchActiveIndex: 1, searchResultsVersion: 1 }))
    expect(handler).toHaveBeenCalledTimes(1)

    // Re-index moved match positions but count/active happen to be identical: the
    // version bump alone must notify (the overlay/label may need a re-read).
    applyState(makeState({ searchCount: 2, searchActiveIndex: 1, searchResultsVersion: 2 }))
    expect(handler).toHaveBeenCalledTimes(2)

    // Byte-identical search fields → no notification.
    applyState(makeState({ searchCount: 2, searchActiveIndex: 1, searchResultsVersion: 2 }))
    expect(handler).toHaveBeenCalledTimes(2)
  })

  it('fires on a stale flip and exposes the flag via searchStateSnapshot', () => {
    const { facade, applyState } = makeTerm()
    const handler = vi.fn()
    facade.onSearchStateChange(handler)

    applyState(makeState({ searchCount: 3, searchActiveIndex: 3, searchResultsStale: true }))
    expect(handler).toHaveBeenCalledTimes(1)
    expect(facade.searchStateSnapshot()).toEqual({
      count: 3,
      activeIndex: 3,
      activeRect: null,
      stale: true,
      incomplete: false,
      markers: { fractions: [], activeFraction: null },
      pending: false
    })

    // The guaranteed trailing refresh clears the flag → one more notification.
    applyState(
      makeState({
        searchCount: 3,
        searchActiveIndex: 3,
        searchResultsStale: false,
        searchResultsVersion: 1
      })
    )
    expect(handler).toHaveBeenCalledTimes(2)
    expect(facade.searchStateSnapshot().stale).toBe(false)
  })

  it('fires on an incomplete flip and exposes the flag via searchStateSnapshot (E9a)', () => {
    const { facade, applyState } = makeTerm()
    const handler = vi.fn()
    facade.onSearchStateChange(handler)

    // The engine truncated the index (eviction / match cap): the flag must reach
    // the snapshot so the count UI can render "N+".
    applyState(makeState({ searchCount: 7, searchActiveIndex: 7, searchResultsIncomplete: true }))
    expect(handler).toHaveBeenCalledTimes(1)
    expect(facade.searchStateSnapshot().incomplete).toBe(true)

    // A later find covers the full buffer → the flip back must notify too.
    applyState(
      makeState({
        searchCount: 7,
        searchActiveIndex: 7,
        searchResultsIncomplete: false,
        searchResultsVersion: 1
      })
    )
    expect(handler).toHaveBeenCalledTimes(2)
    expect(facade.searchStateSnapshot().incomplete).toBe(false)
  })

  it('treats a STATE without the E9a field (older worker) as complete — current behavior', () => {
    const { facade, applyState } = makeTerm()
    const state = makeState({ searchCount: 4, searchActiveIndex: 1 })
    delete (state as Partial<AtermWorkerState>).searchResultsIncomplete
    applyState(state)
    expect(facade.searchStateSnapshot().incomplete).toBe(false)
  })

  it('routes term.search() through the id-correlated query channel (not a command)', () => {
    const { facade, posted, resolveQuery } = makeTerm()
    const term = facade as unknown as {
      search: (q: string, cs: boolean, re?: boolean) => Uint32Array
    }
    expect(term.search('needle', true, false)).toEqual(new Uint32Array(0))
    const query = posted.find((cmd) => cmd.type === 'query')
    expect(query).toMatchObject({ type: 'query', kind: 'searchFind', text: 'needle' })
    // Settle the (fire-and-forget) round-trip so no real 5s timer outlives the test.
    resolveQuery((query as { id: number }).id, null)
  })
})
