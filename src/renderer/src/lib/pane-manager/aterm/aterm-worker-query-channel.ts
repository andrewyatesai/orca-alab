// The id-correlated async query channel for the worker-backed term: serialize / cold
// content reads / fresh selection + link hits the synchronous snapshot facade can't
// answer (they need off-screen history or post-mutation freshness). Each query gets a
// per-call TIMEOUT, and the channel can be disposed so a dropped 'queryResult' (e.g. the
// loader terminated the worker) settles awaiters to a safe null instead of hanging a
// Promise.all at quit (save/hydrate/fork).

import type { AtermWorkerQuery, AtermWorkerRequest } from './aterm-render-worker-protocol'

/** A detected link span returned by the async linkAt query. */
export type AtermWorkerLinkHit = { url: string; kind: number; start_col: number; end_col: number }

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
}

// A dropped 'queryResult' (terminated/wedged worker) must not leave an awaiter hanging;
// settle it to a safe null after this long. Generous so a busy worker's real reply wins.
const QUERY_TIMEOUT_MS = 5000

export function createAtermWorkerQueryChannel(
  post: (cmd: AtermWorkerRequest) => void
): AtermWorkerQueryChannel {
  let nextQueryId = 1
  type Pending = {
    resolve: (value: string | number | boolean | null) => void
    timer: ReturnType<typeof setTimeout>
  }
  const pending = new Map<number, Pending>()

  const settle = (id: number, value: string | number | boolean | null): void => {
    const entry = pending.get(id)
    if (!entry) {
      return
    }
    pending.delete(id)
    clearTimeout(entry.timer)
    entry.resolve(value)
  }

  const send = (
    kind: AtermWorkerQuery['kind'],
    arg?: number,
    arg2?: number
  ): Promise<string | number | boolean | null> =>
    new Promise((resolve) => {
      const id = nextQueryId++
      // Per-query timeout: a never-arriving reply settles to null rather than hang.
      const timer = setTimeout(() => settle(id, null), QUERY_TIMEOUT_MS)
      pending.set(id, { resolve, timer })
      post({ type: 'query', id, kind, arg, arg2 })
    })

  const asString = async (kind: AtermWorkerQuery['kind'], arg?: number): Promise<string> => {
    const v = await send(kind, arg)
    return typeof v === 'string' ? v : ''
  }

  return {
    resolve: settle,
    dispose: () => {
      // Deleting the current key during Map iteration is well-defined (it just won't be
      // revisited), so settle each in-flight query in place.
      for (const id of pending.keys()) {
        settle(id, null)
      }
    },
    serializeAsync: (scrollbackRows) => asString('serialize', scrollbackRows),
    serializeScrollbackAsync: (maxRows) => asString('serializeScrollback', maxRows),
    selectionTextAsync: () => asString('selectionText'),
    linkAtAsync: async (row, col) => {
      const v = await send('linkAt', row, col)
      if (typeof v !== 'string') {
        return null
      }
      try {
        return JSON.parse(v) as AtermWorkerLinkHit
      } catch {
        return null
      }
    }
  }
}
