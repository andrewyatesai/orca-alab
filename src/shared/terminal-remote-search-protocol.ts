// Fed §2.4 remote wire: the `terminal.search` / `terminal.searchContext`
// runtime-RPC contract and the client-side row remap.
//
// Host rows are STABLE engine coordinates (retained-origin + retained index —
// monotonic across eviction, reset only when the host emulator is rebuilt).
// The snapshot reply carries `hostRowAnchor`/`anchorGen` — the stable row of
// the FIRST row serialized into the combined wire snapshot and the generation
// tying the anchor to that exact snapshot. The remap below is the design's
// navigation contract: client row = match.hostRow − hostRowAnchor +
// client-replay origin, allowed ONLY when the anchor generation matches the
// snapshot the client actually replayed — otherwise inline context expansion,
// never a wrong-row jump.

/** Bumped only on breaking wire changes; old hosts are detected by
 *  `method_not_found`, so v1 clients treat ANY known schema >= 1 as usable. */
export const TERMINAL_REMOTE_SEARCH_SCHEMA_VERSION = 1

export type RemoteTerminalSearchMatch = {
  /** Stable host row (see module header). */
  hostRow: number
  /** Char offset into `line` (host is row/col authority; approx across widths). */
  col: number
  len: number
  /** Source-side snippet — scrollback text never ships for matching. */
  line: string
}

export type RemoteTerminalSearchResult = {
  searchSchema: number
  /** False when the host cannot search this pane (no model state, old engine
   *  addon) — the federation controller shows "source unavailable", never an
   *  error. */
  available: boolean
  matches: RemoteTerminalSearchMatch[]
  total: number
  incomplete: boolean
  /** Query generation echoed from the request (cancellation bookkeeping). */
  gen?: number
  /** Host grid width at search time (wrap-width approximation signal). */
  hostCols: number | null
  /** Echoed ONLY when the request's `clientAnchorGen` names an anchor this
   *  host minted for THIS pane within the current emulator lifetime. Absent →
   *  the client must not in-window remap. */
  hostRowAnchor?: number
  anchorGen?: number
  /** Host grid width when the anchored snapshot was serialized. */
  anchorHostCols?: number
}

export type RemoteTerminalSearchContextResult = {
  searchSchema: number
  available: boolean
  lines: string[]
  /** Stable host row of `lines[0]`; null when unavailable. */
  firstHostRow: number | null
}

/** Wire-shape guard for a `terminal.search` result — shared by every client
 *  transport (runtime environment, SSH relay mux) so schema acceptance cannot
 *  drift between routes. */
export function isRemoteTerminalSearchResultShape(
  value: unknown
): value is RemoteTerminalSearchResult {
  if (!value || typeof value !== 'object') {
    return false
  }
  const result = value as Partial<RemoteTerminalSearchResult>
  return (
    typeof result.searchSchema === 'number' &&
    result.searchSchema >= 1 &&
    typeof result.available === 'boolean' &&
    Array.isArray(result.matches) &&
    typeof result.total === 'number' &&
    typeof result.incomplete === 'boolean'
  )
}

/** Wire-shape guard for a `terminal.searchContext` result. */
export function isRemoteTerminalSearchContextResultShape(
  value: unknown
): value is RemoteTerminalSearchContextResult {
  if (!value || typeof value !== 'object') {
    return false
  }
  const result = value as Partial<RemoteTerminalSearchContextResult>
  return (
    typeof result.searchSchema === 'number' &&
    result.searchSchema >= 1 &&
    typeof result.available === 'boolean' &&
    Array.isArray(result.lines)
  )
}

export type RemoteSearchRowRemapInput = {
  /** The match row from a `terminal.search` response. */
  matchHostRow: number
  /** Anchor echoed by that SAME response (absent → no remap). */
  responseAnchor: { hostRowAnchor: number; anchorGen: number; anchorHostCols?: number } | null
  /** Anchor the client recorded from the snapshot it actually REPLAYED. */
  replayedAnchor: { hostRowAnchor: number; anchorGen: number } | null
  /** Client row (engine coordinate) where the snapshot's first row landed. */
  replayOriginRow: number
  /** Rows the client replayed from that snapshot (history + viewport). */
  replayedRowCount: number
  /** Client engine grid width — differing widths flag the jump approximate. */
  clientCols?: number
}

export type RemoteSearchRowRemap =
  /** Jump in-pane to `clientRow`; `approximate` when wrap widths differ (the
   *  host is row-count authority; the client lands on the nearest boundary). */
  | { kind: 'in-window'; clientRow: number; approximate: boolean }
  /** Row is older than the replayed window — inline context expansion. */
  | { kind: 'out-of-window' }
  /** Anchor missing or generation mismatch — the client would be remapping
   *  against a snapshot it didn't replay; inline context expansion. */
  | { kind: 'anchor-mismatch' }

/** The §1 Navigation mapping for remote matches (the critic's mapping hole:
 *  without the snapshot-reply anchor + generation gate, an in-window jump can
 *  land on the wrong row after truncation/eviction/resync). */
export function remapRemoteSearchRow(input: RemoteSearchRowRemapInput): RemoteSearchRowRemap {
  const { responseAnchor, replayedAnchor } = input
  if (!responseAnchor || !replayedAnchor) {
    return { kind: 'anchor-mismatch' }
  }
  if (responseAnchor.anchorGen !== replayedAnchor.anchorGen) {
    return { kind: 'anchor-mismatch' }
  }
  // Both anchors name the same generation, so the same stable coordinate: use
  // the replayed one (identical by construction; defensive equality below).
  if (responseAnchor.hostRowAnchor !== replayedAnchor.hostRowAnchor) {
    return { kind: 'anchor-mismatch' }
  }
  const offset = input.matchHostRow - replayedAnchor.hostRowAnchor
  if (offset < 0) {
    return { kind: 'out-of-window' }
  }
  if (input.replayedRowCount >= 0 && offset >= input.replayedRowCount) {
    // Newer than the replayed window (post-snapshot output): the live client
    // engine has those rows too, but their client rows are append-dependent;
    // clamping into the window would jump to a WRONG row, so stay honest.
    return { kind: 'out-of-window' }
  }
  const approximate =
    typeof input.clientCols === 'number' &&
    typeof input.responseAnchor?.anchorHostCols === 'number' &&
    input.clientCols !== input.responseAnchor.anchorHostCols
  return { kind: 'in-window', clientRow: input.replayOriginRow + offset, approximate }
}
