// The main-thread mirror of the WORKER-owned search state: stamps each posted
// find with a monotonic request generation, derives "pending" from the STATE
// echo, detects search-slice changes between snapshots (incl. generation
// catch-up and marker drift), and fans out the change feed the search UI and
// the scrollbar marker strip subscribe to. Extracted from the worker-backed
// term to keep that file under the line cap.

import { searchMarkerModelsEqual } from './aterm-search-marker-model'
import type { AtermWorkerPaneCommand, AtermWorkerState } from './aterm-render-worker-protocol'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'

export type WorkerSearchMirror = {
  /** Post a find stamped with the next request generation. */
  postFind: (query: string, caseSensitive: boolean, isRegex: boolean) => void
  /** The facade's searchStateSnapshot over the given STATE: `pending` is true while
   *  the newest posted generation is ahead of the STATE's echo (its count is still
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
  post: (cmd: AtermWorkerPaneCommand) => void
): WorkerSearchMirror {
  // Newest find generation POSTED to the worker; STATE echoes the last APPLIED one.
  let postedGeneration = 0
  const listeners = new Set<() => void>()
  return {
    postFind: (query, caseSensitive, isRegex) => {
      postedGeneration += 1
      post({
        type: 'searchFind',
        query,
        caseSensitive,
        isRegex,
        generation: postedGeneration
      })
    },
    snapshot: (state) => ({
      count: state.searchCount,
      activeIndex: state.searchActiveIndex,
      activeRect: state.searchActiveRect,
      markers: state.searchMarkers,
      pending: postedGeneration > state.searchGeneration
    }),
    changed: (prev, next) =>
      next.searchCount !== prev.searchCount ||
      next.searchActiveIndex !== prev.searchActiveIndex ||
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
