// The P1.1 budgeted sliced find: runs a search query through the engine's budgeted
// resumable API in wall-clock-bounded slices, yielding to the worker message loop
// between slices so keystroke echo and NEWER finds interleave; a newer find observed
// between slices cancels the run (cursor dropped, partial matches never surface).
// Split from the worker search state machine to keep that file under the line cap.

import type { EngineHandle } from './aterm-worker-engine-build'

/** A match in absolute-row coords (the engine's native index space). */
export type WorkerMatch = { line: number; startCol: number; length: number }

export function decodeMatches(flat: Uint32Array): WorkerMatch[] {
  const matches: WorkerMatch[] = []
  for (let i = 0; i + 3 <= flat.length; i += 3) {
    matches.push({ line: flat[i], startCol: flat[i + 1], length: flat[i + 2] })
  }
  return matches
}

// Each budgeted engine call targets this much wall-clock work, so the worker message
// loop (keystroke echo, newer finds) runs between slices instead of stalling behind
// one monolithic index build.
export const SEARCH_SLICE_BUDGET_MS = 7
// Row budget the ms target is converted through (adapted from each slice's measured
// cost; the engine clamps 0 to 1). Bounds keep one mis-measured slice from collapsing
// to per-row calls or ballooning back into a blocking search.
const SEARCH_SLICE_INITIAL_ROWS = 4096
const SEARCH_SLICE_MIN_ROWS = 256
const SEARCH_SLICE_MAX_ROWS = 262144
// A content change between slices makes the engine restart the search from row zero
// (its cursor staleness contract). Under sustained streaming that could re-restart
// forever, so after this many restarts the find escalates to full-pass slices.
export const SEARCH_FIND_MAX_RESTARTS = 3
// Row budget of the escalated full pass: far above any policy-capped buffer, so the
// next budgeted call scans EVERY row inside one synchronous call — content cannot
// change mid-call (the worker is single-threaded), so it completes regardless of
// streaming. Same worst case as a blocking one-shot search, but reached through the
// budgeted path with the newer-query cancel check still run at the slice boundary.
export const SEARCH_FULL_PASS_ROW_BUDGET = 1 << 30

/** How a sliced find run talks back to its query: `isCancelled` is polled between
 *  slices (a newer find OBSERVED mid-run cancels this one), `onDone(false)` settles a
 *  cancelled run (the channel answers null, like the supersede skip), `onDone(true)`
 *  reports a completed find whose results are live in the search state. */
export type WorkerSearchFindRun = {
  isCancelled?: () => boolean
  onDone?: (completed: boolean) => void
}

/** At most ONE sliced run is live per runner; `start` requires the previous run to
 *  have been cancelled first (find/clear/dispose all cancel before mutating). */
export type SlicedFindRunner = {
  start: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean,
    generation: number,
    run: WorkerSearchFindRun | undefined
  ) => void
  /** Cancel the in-flight run, if any: clears the slice timer, frees the engine's
   *  partial index, and settles the run's query to null via `onDone(false)`. */
  cancel: () => void
}

export function createSlicedFindRunner(
  handle: EngineHandle,
  /** Adopt a COMPLETED find (matches + generation + engine cost) into search state —
   *  a cancelled run never reaches this, so its partial matches never surface. */
  adoptCompleted: (found: WorkerMatch[], generation: number, costMs: number) => void
): SlicedFindRunner {
  // In-flight sliced find: its cancel() clears the slice timer and settles the run's
  // query to null. Exactly one run can be live; find/clear/dispose supersede it.
  let activeRun: { cancel: () => void } | null = null
  // Rows per budgeted slice, adapted from each slice's measured cost toward
  // SEARCH_SLICE_BUDGET_MS. Persists across finds (buffer depth is stable per pane).
  let sliceRows = SEARCH_SLICE_INITIAL_ROWS

  const start = (
    q: string,
    cs: boolean,
    regex: boolean,
    gen: number,
    run: WorkerSearchFindRun | undefined
  ): void => {
    const searchBudgeted = handle.searchBudgeted
    if (!searchBudgeted) {
      return
    }
    let cursor: number | undefined
    let rowsSeen = 0
    let restarts = 0
    let fullPass = false
    let engineMs = 0
    let sliceTimer: ReturnType<typeof setTimeout> | null = null
    const me = {
      cancel: (): void => {
        if (sliceTimer !== null) {
          clearTimeout(sliceTimer)
          sliceTimer = null
        }
        // Free the engine's partial index now; the superseding find starts clean.
        handle.searchBudgetedCancel?.()
        run?.onDone?.(false)
      }
    }
    activeRun = me
    const finish = (found: WorkerMatch[]): void => {
      activeRun = null
      adoptCompleted(found, gen, engineMs)
      run?.onDone?.(true)
    }
    const slice = (): void => {
      sliceTimer = null
      if (activeRun !== me) {
        return // superseded — its cancel() already settled the query
      }
      // A newer find ARRIVED (even if not yet executed): cancel between slices.
      if (run?.isCancelled?.()) {
        activeRun = null
        me.cancel()
        return
      }
      const t0 = performance.now()
      const step = searchBudgeted(
        q,
        cs,
        regex,
        cursor,
        fullPass ? SEARCH_FULL_PASS_ROW_BUDGET : sliceRows
      )
      const dt = performance.now() - t0
      engineMs += dt
      if (!fullPass) {
        // Adapt rows-per-slice toward the wall-clock budget (bounded both ways).
        sliceRows = Math.min(
          SEARCH_SLICE_MAX_ROWS,
          Math.max(
            SEARCH_SLICE_MIN_ROWS,
            Math.round((sliceRows * SEARCH_SLICE_BUDGET_MS) / Math.max(dt, 0.5))
          )
        )
      }
      if (step.complete) {
        finish(decodeMatches(step.matches))
        return
      }
      // A rows-scanned DROP means the engine restarted (content changed between
      // slices; the stale cursor started over). Bounded: sustained streaming would
      // restart forever, so escalate to full-pass slices — the NEXT budgeted call
      // covers the whole buffer in one synchronous call and thus completes (still
      // preceded by the isCancelled check, unlike a blocking one-shot fallback).
      if (cursor !== undefined && step.rowsFed <= rowsSeen) {
        restarts++
        if (restarts >= SEARCH_FIND_MAX_RESTARTS) {
          fullPass = true
        }
      }
      rowsSeen = step.rowsFed
      cursor = step.cursor
      sliceTimer = setTimeout(slice, 0)
    }
    slice()
  }

  return {
    start,
    cancel: () => {
      if (activeRun !== null) {
        const run = activeRun
        activeRun = null
        run.cancel()
      }
    }
  }
}
