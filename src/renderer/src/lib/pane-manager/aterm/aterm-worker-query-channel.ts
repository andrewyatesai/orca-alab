// The id-correlated async query channel for the worker-backed term: serialize / cold
// content reads / fresh selection + link hits the synchronous snapshot facade can't
// answer (they need off-screen history or post-mutation freshness). Each query gets a
// per-call TIMEOUT, and the channel can be disposed so a dropped 'queryResult' (e.g. the
// loader terminated the worker) settles awaiters to a safe null instead of hanging a
// Promise.all at quit (save/hydrate/fork).

import type { AtermWorkerPaneCommand, AtermWorkerQuery } from './aterm-render-worker-protocol'
import {
  SEARCH_FIND_FLAG_CASE_SENSITIVE,
  SEARCH_FIND_FLAG_REGEX
} from './aterm-worker-search'

/** A detected link span returned by the async linkAt query. */
export type AtermWorkerLinkHit = { url: string; kind: number; start_col: number; end_col: number }

/** What a completed searchFind round-trip reports (the worker's post-find state). */
export type AtermWorkerSearchFindResult = { count: number; activeIndex: number }

/** Worker-only capabilities the worker-backed term facade adds on top of the sync
 *  AtermTerminal surface. Absent on the in-process engine, so the shared selection/link
 *  input handlers detect them at runtime and fall back to the sync path (byte-identical)
 *  when undefined. */
export type AtermWorkerAsyncFacade = {
  /** Fresh selection text via a worker round-trip (the snapshot lags a frame after a
   *  posted selectionWord/Line/Finish). */
  selectionTextAsync: () => Promise<string>
  /** Fresh link hit at a cell via a worker round-trip (sync link_at returns the lagging
   *  snapshot). */
  linkAtAsync: (row: number, col: number) => Promise<AtermWorkerLinkHit | null>
  /** Clear the worker's hover so its next STATE reports no hoverLink/hoverCursor (the
   *  loader drives the canvas cursor from that state on the worker path). */
  clearHover: () => void
  /** Run a find via the id-correlated query channel; resolves `{count, activeIndex}`
   *  once the worker executed it, or null when superseded by a newer find (cancelled
   *  instantly — never blocks on a busy worker), timed out, or disposed. */
  searchFindAsync: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean
  ) => Promise<AtermWorkerSearchFindResult | null>
  /** Latest search match count / 1-based active index / active-match device rect /
   *  cost-gate stale flag from the worker snapshot. The worker owns the engine, so
   *  term.search() can't return matches synchronously; the count UI reads this instead
   *  of the (empty) main-thread controller. */
  searchStateSnapshot: () => {
    count: number
    activeIndex: number
    activeRect: { x: number; y: number; width: number; height: number } | null
    stale: boolean
  }
  /** Advance / step back / clear the worker's active match (the worker owns the match set;
   *  the main-thread searchController is empty on this path, so next/prev/clear must post). */
  searchNext: () => void
  searchPrev: () => void
  searchClear: () => void
  /** Subscribe to worker search-state changes (count/active-index land async after a posted
   *  find/next/prev); returns a disposer. Lets the search UI re-read the count when it lands. */
  onSearchStateChange: (handler: () => void) => () => void
}

export type AtermWorkerQueryChannel = {
  /** Resolve a pending query by id (fed from a 'queryResult' message). */
  resolve: (id: number, value: string | number | boolean | null) => void
  /** Settle EVERY in-flight query to null + clear its timer; call before the worker is
   *  terminated so awaiters can't hang on a reply that will never arrive. */
  dispose: () => void
  serializeAsync: (scrollbackRows?: number) => Promise<string>
  serializeScrollbackAsync: (maxRows?: number) => Promise<string>
  selectionTextAsync: () => Promise<string>
  linkAtAsync: (row: number, col: number) => Promise<AtermWorkerLinkHit | null>
  /** Find via the query channel with request-generation semantics: issuing a new find
   *  settles the previous in-flight one to null IMMEDIATELY (superseded queries never
   *  block newer ones), and its stale result — if the worker still answers — is
   *  discarded by id. Resolves null on supersede/timeout/dispose. */
  searchFindAsync: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean
  ) => Promise<AtermWorkerSearchFindResult | null>
  /** Parse fence: resolves TRUE once the worker has handled every message posted
   *  before it — all prior 'process' bytes parsed AND their auto-replies already
   *  delivered (postMessage ordering). Resolves FALSE when the fence itself timed
   *  out or the channel was disposed (no real 'flush' reply): the worker is merely
   *  behind, so its replayed-query auto-replies (DA1/CPR/OSC) may not have parsed
   *  yet. The replay guard must only treat a TRUE (real-reply) resolution as
   *  parse-certified; a false one keeps the guard held. */
  settleAsync: () => Promise<boolean>
}

// A dropped 'queryResult' (terminated/wedged worker) must not leave an awaiter hanging;
// settle it to a safe null after this long. Generous so a busy worker's real reply wins.
const QUERY_TIMEOUT_MS = 5000

export function createAtermWorkerQueryChannel(
  post: (cmd: AtermWorkerPaneCommand) => void
): AtermWorkerQueryChannel {
  let nextQueryId = 1
  // Queries sent AFTER dispose (worker gone) resolve to null immediately instead of
  // burning the full timeout — a replay guard settling against a torn-down pane
  // would otherwise hold its drop window open for QUERY_TIMEOUT_MS.
  let disposed = false
  // byReply discriminates a real worker 'queryResult' (true) from a timeout/dispose
  // settle (false) — the replay guard's parse-certification depends on it (see
  // settleAsync). value stays the reply payload for the content/serialize queries.
  type QuerySettlement = { value: string | number | boolean | null; byReply: boolean }
  type Pending = {
    resolve: (settlement: QuerySettlement) => void
    timer: ReturnType<typeof setTimeout>
  }
  const pending = new Map<number, Pending>()

  const settle = (
    id: number,
    value: string | number | boolean | null,
    byReply = true
  ): void => {
    const entry = pending.get(id)
    if (!entry) {
      return
    }
    pending.delete(id)
    clearTimeout(entry.timer)
    entry.resolve({ value, byReply })
  }

  // send + the query id it claimed — searchFindAsync needs the id so a NEWER find can
  // settle (cancel) this one by id while it's still in flight.
  const sendTracked = (
    kind: AtermWorkerQuery['kind'],
    arg?: number,
    arg2?: number,
    text?: string
  ): { id: number; promise: Promise<QuerySettlement> } => {
    if (disposed) {
      return { id: 0, promise: Promise.resolve({ value: null, byReply: false }) }
    }
    const id = nextQueryId++
    const promise = new Promise<QuerySettlement>((resolve) => {
      // Per-query timeout: a never-arriving reply settles to null (byReply=false)
      // rather than hang — NOT a real reply, so the fence stays uncertified.
      const timer = setTimeout(() => settle(id, null, false), QUERY_TIMEOUT_MS)
      pending.set(id, { resolve, timer })
      post({ type: 'query', id, kind, arg, arg2, text })
    })
    return { id, promise }
  }

  const send = (
    kind: AtermWorkerQuery['kind'],
    arg?: number,
    arg2?: number
  ): Promise<QuerySettlement> => sendTracked(kind, arg, arg2).promise

  // The in-flight find's query id, or null. ONE slot: the newest find always wins,
  // whichever caller (search UI / addon facade) issued the older one.
  let pendingFindId: number | null = null

  const asString = async (kind: AtermWorkerQuery['kind'], arg?: number): Promise<string> => {
    const { value } = await send(kind, arg)
    return typeof value === 'string' ? value : ''
  }

  return {
    resolve: settle,
    dispose: () => {
      disposed = true
      // Deleting the current key during Map iteration is well-defined (it just won't be
      // revisited), so settle each in-flight query in place. byReply=false: a dispose
      // is not a real reply, so a settleAsync fence resolves uncertified (false).
      for (const id of pending.keys()) {
        settle(id, null, false)
      }
    },
    serializeAsync: (scrollbackRows) => asString('serialize', scrollbackRows),
    serializeScrollbackAsync: (maxRows) => asString('serializeScrollback', maxRows),
    selectionTextAsync: () => asString('selectionText'),
    searchFindAsync: async (query, caseSensitive, isRegex) => {
      // Cancel the superseded in-flight find NOW: its awaiter resolves null instantly
      // (stale result discarded) instead of waiting out a busy worker or the timeout.
      if (pendingFindId !== null) {
        settle(pendingFindId, null, false)
      }
      const flags =
        (caseSensitive ? SEARCH_FIND_FLAG_CASE_SENSITIVE : 0) |
        (isRegex ? SEARCH_FIND_FLAG_REGEX : 0)
      const { id, promise } = sendTracked('searchFind', flags, undefined, query)
      pendingFindId = id
      const { value } = await promise
      if (pendingFindId === id) {
        pendingFindId = null
      }
      if (typeof value !== 'string') {
        return null
      }
      try {
        return JSON.parse(value) as AtermWorkerSearchFindResult
      } catch {
        return null
      }
    },
    settleAsync: async () => {
      // Discriminant: TRUE only when the worker's real 'flush' queryResult arrives —
      // postMessage FIFO then proves every prior 'process' byte parsed AND its
      // auto-replies were already delivered. A 5s timeout or dispose resolves FALSE:
      // an alive-but->5s-behind worker may still parse replayed query bytes
      // (DA1/CPR/OSC) after this, so the replay guard must NOT treat it as certified.
      return (await send('flush')).byReply
    },
    linkAtAsync: async (row, col) => {
      const { value } = await send('linkAt', row, col)
      if (typeof value !== 'string') {
        return null
      }
      try {
        return JSON.parse(value) as AtermWorkerLinkHit
      } catch {
        return null
      }
    }
  }
}
