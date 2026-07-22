// The worker-side search state machine: the single engine lives in the worker, so the
// find/next/prev/clear loop + match index run here (the main-thread search API posts
// commands and reads count/active/rect from the snapshot). Extracted from the worker
// terminal to keep that file under the line cap.

import type { EngineHandle } from './aterm-worker-engine-build'
import type { AtermWorkerState } from './aterm-render-worker-protocol'
import { visibleMatchRange } from './aterm-search-visible-range'
import {
  createSearchMarkerModelCache,
  type AtermSearchMarkerModel
} from './aterm-search-marker-model'
import {
  createSlicedFindRunner,
  decodeMatches,
  type WorkerMatch,
  type WorkerSearchFindRun
} from './aterm-worker-search-sliced-find'

export type { WorkerSearchFindRun } from './aterm-worker-search-sliced-find'

// Cost gate (P6 interim rule until the engine's incremental index lands): a re-index
// whose LAST full rebuild took longer than this refresh tick is not re-run on every
// streaming frame — the stale results are served (flagged) and the trailing timer
// does the guaranteed final re-index.
export const SEARCH_REFRESH_TICK_MS = 100

// searchFind rides the numeric query `arg` as a bitfield (the query wire shape has no
// boolean fields); both ends import these so the encoding can't drift.
export const SEARCH_FIND_FLAG_CASE_SENSITIVE = 1
export const SEARCH_FIND_FLAG_REGEX = 2

/** Decode a searchFind query's wire args (text + flag bits), run it — sliced through
 *  the budgeted engine API when available — and `respond` the post-find state as the
 *  JSON payload the query channel parses (null when the run was cancelled). `respond`
 *  fires synchronously when the find completes in one slice (small buffers / the
 *  one-shot fallback), asynchronously otherwise. */
export function answerSearchFindQuery(
  search: WorkerSearch,
  arg: number | undefined,
  text: string | undefined,
  /** The query id — doubles as the request generation echoed in STATE.searchGeneration. */
  generation = 0,
  isCancelled?: () => boolean,
  respond?: (value: string | null) => void
): void {
  const flags = arg ?? 0
  search.find(
    text ?? '',
    (flags & SEARCH_FIND_FLAG_CASE_SENSITIVE) !== 0,
    (flags & SEARCH_FIND_FLAG_REGEX) !== 0,
    generation,
    {
      isCancelled,
      onDone: (completed) =>
        respond?.(
          completed
            ? JSON.stringify({ count: search.count(), activeIndex: search.activeIndex() })
            : null
        )
    }
  )
}

export type WorkerSearch = {
  /** Run a new query (user-initiated — never cost-gated). `generation` is the
   *  query channel's monotonic request id, echoed in the snapshot so the main side
   *  can flag still-pending results; result correlation itself rides the channel.
   *  With the budgeted engine API the query runs in message-loop-yielding slices
   *  (a NEW find/clear — or `run.isCancelled` — cancels the in-flight run, whose
   *  `run.onDone(false)` then settles its query); results/generation land only on
   *  completion, so a cancelled run's matches never surface. */
  find: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean,
    generation?: number,
    run?: WorkerSearchFindRun
  ) => void
  next: () => void
  prev: () => void
  clear: () => void
  /** Flag the index stale after output changed, WITHOUT re-indexing (a full
   *  O(scrollback) rebuild). The next read/nav re-indexes once — so streaming
   *  many chunks between frames costs one rebuild, not one per chunk. */
  markDirty: () => void
  /** Re-run the active query NOW, preserving the active index (the rare resize
   *  path forces this; streaming uses markDirty + lazy re-index instead). */
  refresh: () => void
  count: () => number
  /** 1-based active match index, or 0 when there are none. */
  activeIndex: () => number
  /** Device-pixel rect of the active match if it's on screen, else null. */
  activeRect: () => AtermWorkerState['searchActiveRect']
  /** Device-pixel rects of ALL on-screen matches (for the main-thread overlay), each
   *  flagged active so the overlay can paint the active one stronger. */
  visibleRects: () => AtermWorkerState['searchMatchRects']
  /** Bumped on every re-index — lets the snapshot diff detect a result-set change
   *  even when count/active happen to be identical (result versioning). */
  resultsVersion: () => number
  /** True while the cost gate is serving results older than the buffer content. */
  resultsStale: () => boolean
  /** Echo of the last find's request generation (0 before any find). */
  generation: () => number
  /** Scrollbar marker model from the FULL sorted match list (bounded buckets);
   *  memoized, so unchanged frames cost a reference compare. */
  markerModel: () => AtermSearchMarkerModel
  /** Cancel the armed trailing-refresh timer (pane dispose). */
  dispose: () => void
}

export function createWorkerSearch(
  handle: EngineHandle,
  getRows: () => number,
  /** Fired after the trailing timer re-indexes (no frame may follow the last content
   *  change, so the owner must post a fresh STATE — the guaranteed final refresh). */
  onAsyncRefresh?: () => void
): WorkerSearch {
  const e = handle.engine
  let matches: WorkerMatch[] = []
  let active = -1
  let query = ''
  let caseSensitive = false
  let isRegex = false
  let dirty = false
  let resultsVersion = 0
  let stale = false
  let lastRebuildMs = 0
  let refreshTimer: ReturnType<typeof setTimeout> | null = null
  let generation = 0
  const markerCache = createSearchMarkerModelCache()

  const cancelRefreshTimer = (): void => {
    if (refreshTimer !== null) {
      clearTimeout(refreshTimer)
      refreshTimer = null
    }
  }

  // Every re-index funnels through here so the cost gate always has a fresh price.
  const runMeasured = (): void => {
    const t0 = performance.now()
    matches = query ? decodeMatches(handle.search(query, caseSensitive, isRegex)) : []
    lastRebuildMs = performance.now() - t0
    resultsVersion++
    dirty = false
    stale = false
  }
  const reindexPreservingActive = (): void => {
    if (!query) {
      return
    }
    runMeasured()
    active = matches.length === 0 ? -1 : Math.min(Math.max(active, 0), matches.length - 1)
  }
  // Trailing refresh: armed when the cost gate skips, fires after a cost-proportional
  // delay (bounds re-index duty cycle ≤ ~50% during streaming) and doubles as the
  // guaranteed FINAL refresh once output stops — no permanently-stale display.
  const armRefreshTimer = (): void => {
    if (refreshTimer !== null) {
      return
    }
    refreshTimer = setTimeout(
      () => {
        refreshTimer = null
        if (dirty && query) {
          reindexPreservingActive()
          onAsyncRefresh?.()
        }
      },
      Math.max(SEARCH_REFRESH_TICK_MS, lastRebuildMs)
    )
  }
  // Re-index at most once since the last markDirty — called before every read/nav
  // so the coalesced rebuild lands on the first access per frame. Cost-gated: an
  // expensive index (rebuild > tick) is NOT rebuilt per frame while streaming;
  // the stale results are served (flagged in the snapshot) until the trailing
  // timer re-indexes.
  const ensureFresh = (): void => {
    if (!dirty) {
      return
    }
    if (!query) {
      dirty = false
      return
    }
    if (lastRebuildMs > SEARCH_REFRESH_TICK_MS) {
      stale = true
      armRefreshTimer()
      return
    }
    reindexPreservingActive()
  }

  // Adopt a COMPLETED find (sliced or one-shot). Everything the legacy synchronous
  // find published lands here in one step, so a cancelled sliced run — which never
  // reaches this — leaves state untouched and its partial matches never surface.
  const completeFind = (found: WorkerMatch[], gen: number, costMs: number): void => {
    matches = found
    lastRebuildMs = costMs
    resultsVersion++
    dirty = false
    stale = false
    generation = gen
    // Select the LAST match (closest to the live bottom), matching the main path.
    active = matches.length > 0 ? matches.length - 1 : -1
    if (active >= 0) {
      e.scroll_search_line_into_view(matches[active].line)
    }
  }

  // The P1.1 sliced find runner: budgeted engine calls that yield to the worker
  // message loop between slices; a cancelled run never reaches completeFind.
  const slicedFind = createSlicedFindRunner(handle, completeFind)

  return {
    find: (q, cs, regex, gen = 0, run) => {
      // A new find supersedes any in-flight sliced run (its query settles to null —
      // the main side already cancelled that promise when it issued this find).
      slicedFind.cancel()
      query = q
      caseSensitive = cs
      isRegex = regex
      dirty = false
      cancelRefreshTimer()
      if (!q) {
        generation = gen
        matches = []
        active = -1
        stale = false
        resultsVersion++
        handle.searchBudgetedCancel?.()
        run?.onDone?.(true)
        return
      }
      if (!handle.searchBudgeted) {
        // Artifact-skew fallback (engine without the budgeted API): the legacy
        // blocking one-shot find.
        generation = gen
        runMeasured()
        active = matches.length > 0 ? matches.length - 1 : -1
        if (active >= 0) {
          e.scroll_search_line_into_view(matches[active].line)
        }
        run?.onDone?.(true)
        return
      }
      slicedFind.start(q, cs, regex, gen, run)
    },
    next: () => {
      ensureFresh()
      if (matches.length > 0) {
        active = (active + 1 + matches.length) % matches.length
        e.scroll_search_line_into_view(matches[active].line)
      }
    },
    prev: () => {
      ensureFresh()
      if (matches.length > 0) {
        active = (active - 1 + matches.length) % matches.length
        e.scroll_search_line_into_view(matches[active].line)
      }
    },
    clear: () => {
      slicedFind.cancel()
      handle.searchBudgetedCancel?.()
      matches = []
      active = -1
      query = ''
      caseSensitive = false
      isRegex = false
      dirty = false
      stale = false
      resultsVersion++
      cancelRefreshTimer()
    },
    markDirty: () => {
      dirty = true
    },
    refresh: () => {
      reindexPreservingActive()
      dirty = false
    },
    count: () => {
      ensureFresh()
      return matches.length
    },
    activeIndex: () => {
      ensureFresh()
      return active >= 0 ? active + 1 : 0
    },
    activeRect: () => {
      ensureFresh()
      return active >= 0 ? rectFor(matches[active]) : null
    },
    visibleRects: () => {
      ensureFresh()
      const rects: NonNullable<AtermWorkerState['searchMatchRects']> = []
      if (matches.length === 0) {
        return rects
      }
      // Only probe the on-screen band (matches are line-sorted) — the previous full
      // scan was O(all matches) per frame.
      const firstLine = e.search_display_origin - e.display_offset
      const { start, end } = visibleMatchRange(matches, firstLine, firstLine + getRows())
      for (let i = start; i < end; i++) {
        const rect = rectFor(matches[i])
        if (rect) {
          rects.push({ ...rect, active: i === active })
        }
      }
      return rects
    },
    resultsVersion: () => resultsVersion,
    resultsStale: () => stale,
    generation: () => generation,
    markerModel: () => {
      ensureFresh()
      // Re-base by the oldest retained row: ring eviction keeps absolute rows
      // growing, so raw lines would pin every marker to the bottom of the track.
      const firstLine = e.search_display_origin - e.base_y
      return markerCache(matches, active, firstLine, e.base_y + getRows())
    },
    dispose: () => {
      // Settle any in-flight sliced find BEFORE the engine is freed (its cancel
      // touches the engine's budgeted state), then drop the trailing timer.
      slicedFind.cancel()
      cancelRefreshTimer()
    }
  }

  // Absolute match line → on-screen device-pixel rect, or null when scrolled off
  // (the SAME mapping paintAtermSearchHighlights uses: search_display_origin +
  // display_offset).
  function rectFor(m: WorkerMatch): { x: number; y: number; width: number; height: number } | null {
    const displayRow = m.line - e.search_display_origin + e.display_offset
    if (displayRow < 0 || displayRow >= getRows()) {
      return null
    }
    return {
      x: m.startCol * e.cell_width,
      y: displayRow * e.cell_height,
      width: m.length * e.cell_width,
      height: e.cell_height
    }
  }
}
