// The federated-search result model + source-adapter contract
// (FEDERATED-SEARCH-DESIGN §1 result model, §2 adapter contract). The controller
// merges per-pane batches from every adapter into pane groups; result identity is
// SOURCE-KEYED — daemon-backed results by sessionId, live-only results by
// paneRef.leafId — so a live pane and its daemon depth extension form ONE group.

/** Where a pane lives when it resolves (absent for dead daemon sessions). */
export type FederatedPaneRef = {
  tabId: string
  leafId: string
  /** makePaneKey identity (tabId:leafId) — the live-group key. */
  paneKey: string
  worktreeId: string | null
  title: string | null
}

export type FederatedResultSource = 'live' | 'hidden' | 'parked' | 'daemon-history' | 'remote'

export type FederatedMatch = {
  /** Engine absolute-row coordinate; for persisted/remote sources a row within
   *  that source's stream, flagged approximate by the batch. */
  absRow: number
  col: number
  len: number
  /** Span-marked matched line text, produced SOURCE-SIDE — null on engine pins
   *  without the summary binding (count-only degradation). */
  snippet: string | null
  /** Remote nearest-row-boundary jump (§2.4): true when host/client wrap widths
   *  differ (or the host width was unverifiable), so the client row is the whole
   *  nearest boundary rather than an exact position. Undefined = exact. */
  approximate?: boolean
}

/** One source's results for one pane/session, streamed as it completes. */
export type FederatedPaneBatch = {
  /** ABSENT when no pane resolves (dead daemon session). */
  paneRef?: FederatedPaneRef
  /** Daemon session identity — THE merge key for live-vs-daemon dedup (§2.3). */
  sessionId: string | null
  source: FederatedResultSource
  matches: FederatedMatch[]
  total: number
  incomplete: boolean
  /** Log-batch append time / last-activity time, rendered with a "~" prefix. */
  approxTime: number | null
  /** Daemon rows older than the live window (§2.3): merges into the live group,
   *  badged as history depth, navigated via inline context expansion. */
  depthExtension?: boolean
  /** §4 admission outcome: 'over-budget' = index refused, no unindexed reader
   *  (honest empty); 'linear-scan' = degraded to the bounded unindexed scan
   *  (real, possibly-incomplete matches — never a silent no-results). */
  degraded?: 'none' | 'over-budget' | 'linear-scan'
}

export type FederatedQueryOpts = {
  caseSensitive: boolean
  isRegex: boolean
}

/** A source adapter: streams batches via emit, resolves when its fan-out is done.
 *  (The design sketches AsyncIterable; emit-callback is the equivalent shape the
 *  controller's incremental re-rank consumes directly.) Adapters MUST check gen
 *  before emitting — a stale-generation batch must never render. */
export type SearchSourceAdapter = {
  query: (
    query: string,
    opts: FederatedQueryOpts,
    gen: number,
    maxPerPane: number,
    emit: (batch: FederatedPaneBatch) => void
  ) => Promise<void>
  cancel: (gen: number) => void
}

/** Per-group cap on rendered matches (§1: top-K with "+N more"). */
export const FEDERATED_TOP_K_MATCHES = 50

/** Debounce after the last keystroke before a query fans out (§1 liveness). */
export const FEDERATED_QUERY_DEBOUNCE_MS = 75

/** §1 smart-case: case-insensitive until the query contains an uppercase letter;
 *  the explicit toggle forces sensitivity outright (applies to regex too, the
 *  ripgrep --smart-case convention). */
export function federatedEffectiveCaseSensitive(
  query: string,
  caseSensitiveToggle: boolean
): boolean {
  return caseSensitiveToggle || /\p{Lu}/u.test(query)
}
