// The main-thread mirror of the WORKER-owned search state: derives "pending" by
// comparing the newest find issued on the query channel (finds ride the id-correlated
// 'searchFind' query, so the monotonic query id IS the request generation) against the
// STATE's searchGeneration echo, detects search-slice changes between snapshots (incl.
// generation catch-up and marker drift), and fans out the change feed the search UI
// and the scrollbar marker strip subscribe to. Extracted from the worker-backed term
// to keep that file under the line cap.

import { searchMarkerModelsEqual } from './aterm-search-marker-model'
import type { AtermWorkerState } from './aterm-render-worker-protocol'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'

export type WorkerSearchMirror = {
  /** The facade's searchStateSnapshot over the given STATE: `pending` is true while
   *  the newest issued find id is ahead of the STATE's echo (its count is still
   *  the previous query's). */
  snapshot: (state: AtermWorkerState) => ReturnType<AtermWorkerAsyncFacade['searchStateSnapshot']>
  /** Whether the search slice differs between two STATEs. Generation catch-up must
   *  count even with identical totals (query "a" → "ab") so the pending label
   *  resolves; marker drift must count so the scrollbar strip repaints when
   *  streaming output shifts fractions. */
  changed: (prev: AtermWorkerState, next: AtermWorkerState) => boolean
  /** Fire the change feed (the term calls this when changed() said so). */
  notify: () => void
  /** Subscribe to the change feed; returns a disposer. */
  subscribe: (handler: () => void) => () => void
  /** Drop all subscribers (pane dispose). */
  clear: () => void
}

export function createWorkerSearchMirror(
  /** Newest searchFind query id issued on the channel (the request generation). */
  latestFindId: () => number
): WorkerSearchMirror {
  const listeners = new Set<() => void>()
  return {
    snapshot: (state) => ({
      count: state.searchCount,
      activeIndex: state.searchActiveIndex,
      activeRect: state.searchActiveRect,
      stale: state.searchResultsStale,
      // Coerce: a STATE from a worker predating the E9a plumbing lacks the field.
      incomplete: state.searchResultsIncomplete === true,
      markers: state.searchMarkers,
      pending: latestFindId() > state.searchGeneration
    }),
    changed: (prev, next) =>
      next.searchCount !== prev.searchCount ||
      next.searchActiveIndex !== prev.searchActiveIndex ||
      // Result versioning: a re-index can change match POSITIONS with identical
      // count/active, and the stale flag must reach the search UI's indicator.
      next.searchResultsVersion !== prev.searchResultsVersion ||
      next.searchResultsStale !== prev.searchResultsStale ||
      // An incomplete flip must reach the "N+" count label like the stale flag does.
      next.searchResultsIncomplete !== prev.searchResultsIncomplete ||
      // Generation catch-up resolves the pending label even with identical totals.
      next.searchGeneration !== prev.searchGeneration ||
      !searchMarkerModelsEqual(next.searchMarkers, prev.searchMarkers),
    notify: () => listeners.forEach((fn) => fn()),
    subscribe: (handler) => {
      listeners.add(handler)
      return () => void listeners.delete(handler)
    },
    clear: () => listeners.clear()
  }
}
