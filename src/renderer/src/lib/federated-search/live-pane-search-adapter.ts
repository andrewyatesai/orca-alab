// The live/hidden local-pane source adapter (FEDERATED-SEARCH-DESIGN §2.1):
// worker-path panes ride ONE worker-scoped federatedFind (serial fan-out +
// admission control run IN the worker); in-process-fallback panes are scanned on
// the main thread through the same E-6 slice budget across idle callbacks — an
// unbudgeted call here would stall the UI thread itself.

import {
  FEDERATED_INDEX_BYTES_PER_LINE,
  FEDERATED_WORKER_INDEX_BUDGET_BYTES
} from '../pane-manager/aterm/aterm-worker-federated-find'
import { runFederatedPaneScan } from '../pane-manager/aterm/aterm-federated-budgeted-scan'
import type { AtermFederatedSearchTarget } from '../pane-manager/aterm/aterm-federated-search-target'
import type {
  AtermWorkerFederatedCommand,
  AtermWorkerFederatedEvent
} from '../pane-manager/aterm/aterm-worker-federated-protocol'
import type {
  FederatedPaneBatch,
  FederatedPaneRef,
  SearchSourceAdapter
} from './federated-search-model'

/** One discovered live/hidden pane, priority inputs included. */
export type DiscoveredLivePane = {
  paneRef: FederatedPaneRef
  visible: boolean
  focused: boolean
  /** Daemon session backing this pane (the dedup merge key), when known. */
  sessionId: string | null
  /** Last-activity time for approxTime + recency ordering (0 = unknown). */
  lastOutputAt: number
  target: AtermFederatedSearchTarget
}

// A worker retire mid-run stops the event stream; settle the fan-out instead of
// hanging the palette's pending state forever.
export const LIVE_FANOUT_TIMEOUT_MS = 15_000

/** §2.1 priority order: focused → visible → recency of last output. */
export function orderLivePanes(panes: readonly DiscoveredLivePane[]): DiscoveredLivePane[] {
  const rank = (p: DiscoveredLivePane): number => (p.focused ? 0 : p.visible ? 1 : 2)
  return [...panes].sort((a, b) => rank(a) - rank(b) || b.lastOutputAt - a.lastOutputAt)
}

export function createLivePaneSearchAdapter(deps: {
  discoverPanes: () => DiscoveredLivePane[]
  postFederated: (cmd: AtermWorkerFederatedCommand) => boolean
  subscribeFederated: (handler: (event: AtermWorkerFederatedEvent) => void) => () => void
  /** Main-thread yield seam for the in-process fallback (idle callbacks in
   *  production; immediate in tests). */
  yieldIdle?: (next: () => void) => void
  timeoutMs?: number
}): SearchSourceAdapter {
  const yieldIdle =
    deps.yieldIdle ??
    ((next: () => void): void => {
      // requestIdleCallback keeps fallback slices out of interaction frames;
      // setTimeout is the cross-environment fallback (workers/jsdom lack rIC).
      if (typeof requestIdleCallback === 'function') {
        requestIdleCallback(() => next())
      } else {
        setTimeout(next, 0)
      }
    })
  const cancelledGens = new Set<number>()

  const query: SearchSourceAdapter['query'] = async (q, opts, gen, maxPerPane, emit) => {
    const panes = orderLivePanes(deps.discoverPanes())
    const isCancelled = (): boolean => cancelledGens.has(gen)
    const toBatch = (
      pane: DiscoveredLivePane,
      matches: FederatedPaneBatch['matches'],
      total: number,
      incomplete: boolean,
      degraded: 'none' | 'over-budget'
    ): FederatedPaneBatch => ({
      paneRef: pane.paneRef,
      sessionId: pane.sessionId,
      source: pane.focused || pane.visible ? 'live' : 'hidden',
      matches,
      total,
      incomplete,
      approxTime: pane.lastOutputAt || null,
      degraded
    })

    // ── Worker-path panes: ONE postMessage, streamed batches back ──────────────
    const workerPanes = panes.filter((p) => p.target.kind === 'worker')
    const byWorkerPaneId = new Map<number, DiscoveredLivePane>()
    for (const pane of workerPanes) {
      if (pane.target.kind === 'worker') {
        byWorkerPaneId.set(pane.target.workerPaneId, pane)
      }
    }
    const workerRun =
      workerPanes.length === 0
        ? Promise.resolve()
        : new Promise<void>((resolve) => {
            let settled = false
            let unsubscribe: () => void = () => undefined
            const settle = (): void => {
              if (!settled) {
                settled = true
                clearTimeout(timer)
                unsubscribe()
                resolve()
              }
            }
            const timer = setTimeout(settle, deps.timeoutMs ?? LIVE_FANOUT_TIMEOUT_MS)
            unsubscribe = deps.subscribeFederated((event) => {
              if (event.gen !== gen) {
                return
              }
              if (event.type === 'federatedBatch') {
                const pane = byWorkerPaneId.get(event.paneId)
                if (pane && !isCancelled()) {
                  emit(toBatch(pane, event.matches, event.total, event.incomplete, event.degraded))
                }
                return
              }
              settle()
            })
            const posted = deps.postFederated({
              type: 'federatedFind',
              gen,
              query: q,
              caseSensitive: opts.caseSensitive,
              isRegex: opts.isRegex,
              maxPerPane,
              panes: workerPanes.map((pane) => ({
                paneId: pane.target.kind === 'worker' ? pane.target.workerPaneId : -1,
                visible: pane.visible || pane.focused
              }))
            })
            if (!posted) {
              settle() // no live worker → no worker-path panes can answer
            }
          })

    // ── In-process fallback panes: serial main-thread scan, idle-sliced ────────
    const inProcessPanes = panes.filter((p) => p.target.kind === 'in-process')
    const inProcessRun = (async () => {
      // §4 admission is CUMULATIVE (the worker walk's rule, mirrored): visible
      // panes keep their warm index after scanning, so their estimates stay
      // counted — later panes are refused once the collective retained bytes
      // would breach the budget, not judged one pane at a time.
      let residentEstimateBytes = 0
      for (const pane of inProcessPanes) {
        if (isCancelled() || pane.target.kind !== 'in-process') {
          return
        }
        const target = pane.target
        const estimate = (target.baseY() + target.rows()) * FEDERATED_INDEX_BYTES_PER_LINE
        if (residentEstimateBytes + estimate > FEDERATED_WORKER_INDEX_BUDGET_BYTES) {
          // Honest degradation: too deep to index within the remaining budget.
          emit(toBatch(pane, [], 0, true, 'over-budget'))
          continue
        }
        await new Promise<void>((resolve) => {
          runFederatedPaneScan({
            engine: target.engine,
            query: q,
            caseSensitive: opts.caseSensitive,
            isRegex: opts.isRegex,
            maxMatches: maxPerPane,
            isCancelled,
            yieldSlice: yieldIdle,
            onDone: (result) => {
              if (result && !isCancelled()) {
                emit(toBatch(pane, result.matches, result.total, result.incomplete, 'none'))
              }
              if (pane.visible || pane.focused) {
                // Visible panes keep the warm index (their find bar benefits) —
                // it stays counted against this run's resident budget.
                residentEstimateBytes += estimate
              } else {
                // Immediate eviction for non-visible panes (§4).
                target.engine.searchBudgetedCancel?.()
              }
              resolve()
            }
          })
        })
      }
    })()

    await Promise.all([workerRun, inProcessRun])
  }

  return {
    query,
    cancel: (gen) => {
      cancelledGens.add(gen)
      deps.postFederated({ type: 'federatedCancel', gen })
      // Bound the set: generations are monotonic, so old entries can drop once
      // a newer one is cancelled too.
      if (cancelledGens.size > 32) {
        const oldest = Math.min(...cancelledGens)
        cancelledGens.delete(oldest)
      }
    }
  }
}
