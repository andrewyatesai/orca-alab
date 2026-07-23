// The renderer main-thread FederatedSearchController (FEDERATED-SEARCH-DESIGN §2):
// owns the generation counter, fans out to source adapters, merges/ranks batches
// incrementally, and feeds the palette. Every keystroke bumps the generation;
// batches are gen-checked at delivery so a stale-generation batch never renders;
// Esc/close cancels all in-flight source queries.

import {
  mergeFederatedBatch,
  orderFederatedGroups,
  type FederatedGroupOrderContext,
  type FederatedResultGroup
} from './federated-search-grouping'
import type {
  FederatedPaneBatch,
  FederatedQueryOpts,
  SearchSourceAdapter
} from './federated-search-model'
import { FEDERATED_TOP_K_MATCHES } from './federated-search-model'
import type { FederatedDepthExtension } from './federated-session-dedup'

export type FederatedSearchSnapshot = {
  gen: number
  query: string
  opts: FederatedQueryOpts
  groups: FederatedResultGroup[]
  /** True while at least one adapter's fan-out is still streaming. */
  pending: boolean
}

export type FederatedSearchController = {
  /** Bump the generation and fan the query out; an empty query just cancels. */
  setQuery: (query: string, opts: FederatedQueryOpts) => void
  /** Cancel all in-flight source queries (Esc / palette close). */
  cancel: () => void
  subscribe: (listener: () => void) => () => void
  snapshot: () => FederatedSearchSnapshot
  dispose: () => void
}

export function createFederatedSearchController(deps: {
  adapters: readonly SearchSourceAdapter[]
  orderContext: () => FederatedGroupOrderContext
  /** Depth-extension cutoffs for the current fan-out (recomputed per query by
   *  the adapter layer; the controller enforces them at merge, defense in depth). */
  depthExtensions?: () => readonly FederatedDepthExtension[]
  maxPerPane?: number
}): FederatedSearchController {
  const listeners = new Set<() => void>()
  let gen = 0
  let query = ''
  let opts: FederatedQueryOpts = { caseSensitive: false, isRegex: false }
  let groups = new Map<string, FederatedResultGroup>()
  let pendingAdapters = 0
  let disposed = false
  // Snapshot identity is stable between notifications (useSyncExternalStore
  // requires a cached getSnapshot; a fresh object per read would spin renders).
  let cachedSnapshot: FederatedSearchSnapshot | null = null

  const notify = (): void => {
    cachedSnapshot = null
    for (const listener of listeners) {
      listener()
    }
  }

  const cancelInFlight = (): void => {
    const cancelled = gen
    for (const adapter of deps.adapters) {
      try {
        adapter.cancel(cancelled)
      } catch {
        // One source's cancel failure must not strand the others mid-flight.
      }
    }
  }

  const cutoffFor = (batch: FederatedPaneBatch): number | undefined => {
    if (!batch.depthExtension || batch.sessionId === null) {
      return undefined
    }
    const extension = deps.depthExtensions?.().find((entry) => entry.sessionId === batch.sessionId)
    return extension?.cutoffRow
  }

  return {
    setQuery: (nextQuery, nextOpts) => {
      if (disposed) {
        return
      }
      cancelInFlight()
      gen++
      query = nextQuery
      opts = nextOpts
      groups = new Map()
      pendingAdapters = 0
      if (nextQuery === '') {
        notify()
        return
      }
      const thisGen = gen
      pendingAdapters = deps.adapters.length
      for (const adapter of deps.adapters) {
        void adapter
          .query(
            nextQuery,
            nextOpts,
            thisGen,
            deps.maxPerPane ?? FEDERATED_TOP_K_MATCHES,
            (batch) => {
              // Generation gate: a stale batch (superseded query) never renders.
              if (disposed || thisGen !== gen) {
                return
              }
              mergeFederatedBatch(groups, batch, cutoffFor(batch))
              notify()
            }
          )
          .catch(() => undefined) // a failing source degrades to "no results", never throws
          .finally(() => {
            if (!disposed && thisGen === gen) {
              pendingAdapters--
              notify()
            }
          })
      }
      notify()
    },
    cancel: () => {
      if (disposed) {
        return
      }
      cancelInFlight()
      gen++
      pendingAdapters = 0
      notify()
    },
    subscribe: (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    snapshot: () => {
      if (!cachedSnapshot) {
        cachedSnapshot = {
          gen,
          query,
          opts,
          groups: orderFederatedGroups(groups.values(), deps.orderContext()),
          pending: pendingAdapters > 0
        }
      }
      return cachedSnapshot
    },
    dispose: () => {
      if (!disposed) {
        cancelInFlight()
        disposed = true
        listeners.clear()
      }
    }
  }
}
