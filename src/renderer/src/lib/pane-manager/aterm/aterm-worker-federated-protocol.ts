// Wire contract for the WORKER-SCOPED federated find (FEDERATED-SEARCH-DESIGN §2.1):
// ONE postMessage fans a query across every worker-hosted pane engine — walked
// SERIALLY in the main-thread-supplied priority order under the §4 memory-admission
// rule — and per-pane result batches stream back as worker-scoped events. No paneId
// envelope: the run is worker-global; each batch names its pane explicitly.
//
// This file is types-only so the worker and the main-side manager/adapter share one
// contract without importing each other's runtime.

/** One federated match in the engine's absolute-row coordinate space. */
export type AtermFederatedMatch = {
  absRow: number
  col: number
  len: number
  /** Matched line text (E-1 `search_summary`), or null on engine pins without the
   *  summary binding — the count-only degradation shape (bare triplets). */
  snippet: string | null
}

export type AtermWorkerFederatedFindPane = {
  paneId: number
  /** Visible panes keep their warm index after the scan (find-bar TTL semantics);
   *  non-visible panes are evicted immediately after their scan (§4 admission). */
  visible: boolean
}

/** Worker-scoped command: run one federated query over the listed panes. A new
 *  find cancels any in-flight run regardless of gen (newest always wins). */
export type AtermWorkerFederatedFind = {
  type: 'federatedFind'
  /** Controller generation token: stale-gen batches are dropped on arrival. */
  gen: number
  query: string
  caseSensitive: boolean
  isRegex: boolean
  /** Per-pane cap on streamed matches (the full per-pane total still reports). */
  maxPerPane: number
  /** Priority-ordered serial walk (focused → visible → recency), computed
   *  main-side — the worker has no tab/focus model of its own. */
  panes: AtermWorkerFederatedFindPane[]
}

/** Worker-scoped command: cancel the in-flight run if it carries this gen. */
export type AtermWorkerFederatedCancel = { type: 'federatedCancel'; gen: number }

/** One pane's completed scan, streamed as soon as that pane finishes. */
export type AtermWorkerFederatedBatch = {
  type: 'federatedBatch'
  gen: number
  paneId: number
  /** Newest-first (§1 grouping contract: highest absRow first), capped at maxPerPane. */
  matches: AtermFederatedMatch[]
  /** Full match count for the pane (matches.length ≤ total under the cap). */
  total: number
  /** Index eviction / match cap / streaming-restart settle truncated the results. */
  incomplete: boolean
  /** 'over-budget': the §4 admission rule refused to index this pane (estimated
   *  index bytes would breach the worker budget) — total is 0 and honest. */
  degraded: 'none' | 'over-budget'
}

/** The run finished (all panes scanned) or was cancelled part-way. */
export type AtermWorkerFederatedDone = {
  type: 'federatedDone'
  gen: number
  cancelled: boolean
}

export type AtermWorkerFederatedCommand = AtermWorkerFederatedFind | AtermWorkerFederatedCancel
export type AtermWorkerFederatedEvent = AtermWorkerFederatedBatch | AtermWorkerFederatedDone
