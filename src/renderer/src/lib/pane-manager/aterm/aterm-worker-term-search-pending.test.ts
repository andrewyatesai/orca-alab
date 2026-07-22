import { describe, expect, it, vi } from 'vitest'
import { createWorkerBackedTerm } from './aterm-worker-term'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'
import type {
  AtermWorkerPaneCommand,
  AtermWorkerQuery,
  AtermWorkerState
} from './aterm-render-worker-protocol'

// The worker-backed term must correlate issued finds with the STATE echo
// (searchGeneration): finds ride the id-correlated query channel, so the monotonic
// query id IS the request generation. Until the worker echoes the newest issued id,
// the snapshot count is the PREVIOUS query's, so searchStateSnapshot flags `pending`
// and the change feed must fire on the catch-up even when the count is identical.

function makeWorkerState(overrides: Partial<AtermWorkerState> = {}): AtermWorkerState {
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

type SearchFacade = AtermWorkerAsyncFacade & {
  search: (query: string, caseSensitive: boolean, isRegex?: boolean) => Uint32Array
}

function makeBackedTerm(): {
  facade: SearchFacade
  applyState: (state: AtermWorkerState) => void
  finds: AtermWorkerQuery[]
} {
  const finds: AtermWorkerQuery[] = []
  const backed = createWorkerBackedTerm({
    post: (cmd: AtermWorkerPaneCommand) => {
      if (cmd.type === 'query' && cmd.kind === 'searchFind') {
        finds.push(cmd)
      }
    },
    initial: makeWorkerState()
  })
  return {
    facade: backed.term as unknown as SearchFacade,
    applyState: backed.applyState,
    finds
  }
}

describe('worker-backed term search pending state', () => {
  it('issues a monotonic find id and pends until the STATE echo catches up', () => {
    const { facade, applyState, finds } = makeBackedTerm()
    expect(facade.searchStateSnapshot().pending).toBe(false)

    facade.search('foo', false)
    facade.search('foob', false)
    expect(finds.map((f) => f.id)).toEqual([1, 2])

    // First find's results land — but a newer find is still in flight: pending,
    // and the stale count (previous query's) is what the label approximates.
    applyState(
      makeWorkerState({
        searchGeneration: 1,
        searchCount: 5,
        searchActiveIndex: 5
      })
    )
    expect(facade.searchStateSnapshot()).toMatchObject({
      count: 5,
      pending: true
    })

    applyState(
      makeWorkerState({
        searchGeneration: 2,
        searchCount: 3,
        searchActiveIndex: 3
      })
    )
    expect(facade.searchStateSnapshot()).toMatchObject({
      count: 3,
      pending: false
    })
  })

  it('fires the change feed on generation catch-up even when the count is unchanged', () => {
    const { facade, applyState } = makeBackedTerm()
    const handler = vi.fn()
    facade.onSearchStateChange(handler)

    facade.search('a', false)
    applyState(
      makeWorkerState({
        searchGeneration: 1,
        searchCount: 5,
        searchActiveIndex: 5
      })
    )
    expect(handler).toHaveBeenCalledTimes(1)

    // "a" → "ab": same total, different query — the label must still resolve
    // from "~5, searching…" to "5 / 5", so the echo alone must notify.
    facade.search('ab', false)
    applyState(
      makeWorkerState({
        searchGeneration: 2,
        searchCount: 5,
        searchActiveIndex: 5
      })
    )
    expect(handler).toHaveBeenCalledTimes(2)
  })

  it('fires the change feed when only the marker model drifts (streaming output)', () => {
    const { facade, applyState } = makeBackedTerm()
    const handler = vi.fn()
    facade.onSearchStateChange(handler)

    applyState(
      makeWorkerState({
        searchCount: 2,
        searchActiveIndex: 2,
        searchMarkers: { fractions: [0.1, 0.9], activeFraction: 0.9 }
      })
    )
    expect(handler).toHaveBeenCalledTimes(1)
    expect(facade.searchStateSnapshot().markers.fractions).toEqual([0.1, 0.9])

    // Same count/active, shifted fractions (buffer grew) → still a notify.
    applyState(
      makeWorkerState({
        searchCount: 2,
        searchActiveIndex: 2,
        searchMarkers: { fractions: [0.05, 0.45], activeFraction: 0.45 }
      })
    )
    expect(handler).toHaveBeenCalledTimes(2)

    // Value-equal model → no spurious notify.
    applyState(
      makeWorkerState({
        searchCount: 2,
        searchActiveIndex: 2,
        searchMarkers: { fractions: [0.05, 0.45], activeFraction: 0.45 }
      })
    )
    expect(handler).toHaveBeenCalledTimes(2)
  })
})
