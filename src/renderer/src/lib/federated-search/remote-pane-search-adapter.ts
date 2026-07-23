// The remote/SSH pane source adapter (FEDERATED-SEARCH-DESIGN §2.4, Wave-5 5C):
// the deferred federation entry for remote panes over the LANDED 5B wire
// (`terminal.search` + hostRowAnchor/anchorGen, runtime route and SSH mux route).
// Host rows are STABLE host-absolute coordinates; this adapter remaps each match
// into the client engine's row space with the snapshot-anchor generation gate
// (terminal-remote-search-protocol.remapRemoteSearchRow) so an in-window match
// jumps to the right client row, a width-mismatched anchor lands on the nearest
// row boundary flagged approximate, and an anchor-mismatch/out-of-window match is
// carried as "deeper history" (host-side inline expansion, never a wrong-row
// jump). Remote sources stream in behind local results and never block them.

import {
  remapRemoteSearchRow,
  type RemoteTerminalSearchResult
} from '../../../../shared/terminal-remote-search-protocol'
import {
  FEDERATED_TOP_K_MATCHES,
  type FederatedMatch,
  type FederatedPaneBatch,
  type FederatedPaneRef,
  type FederatedQueryOpts,
  type SearchSourceAdapter
} from './federated-search-model'

/** The anchor a client recorded from the snapshot it actually replayed. */
export type RemoteReplayedAnchor = { hostRowAnchor: number; anchorGen: number }

/** One discovered remote pane, with the client-side replay geometry the remap
 *  needs. Discovery reads store tab/worktree state (which knows remote providers)
 *  plus the multiplexer's `getReplayedHostAnchor()` (§2.4 client side). */
export type DiscoveredRemotePane = {
  paneRef: FederatedPaneRef
  /** Host session identity when known (dedup merge key); null otherwise. */
  sessionId: string | null
  /** Anchor of the snapshot the client actually replayed; null → no in-window
   *  remap is possible (every match is deeper-history inline). */
  replayedAnchor: RemoteReplayedAnchor | null
  /** Client engine row where the replayed snapshot's first row landed. */
  replayOriginRow: number
  /** Rows the client replayed (history + viewport) from that snapshot. */
  replayedRowCount: number
  /** Client engine grid width — differing widths flag the jump approximate. */
  clientCols: number | null
  /** Last-activity time for approxTime + ordering (0 = unknown). */
  lastOutputAt: number
  /** Runtime-environment selector that routes `terminal.search` to this pane's
   *  host (the same selector the multiplexer subscribed with). */
  environmentId: string
  /** Host-side terminal id the `terminal.search` request names. */
  hostTerminalId: string
}

/** The 5B wire call for one pane (runtime route or SSH mux route), already
 *  degradation-aware: returns the parsed result, or null when the host/pane is
 *  unsearchable (old host, no model, unreachable) — the adapter shows the source
 *  as absent, never an error. `signal` carries cancellation to the host. */
export type RemoteSearchCall = (
  pane: DiscoveredRemotePane,
  request: {
    query: string
    caseSensitive: boolean
    isRegex: boolean
    maxMatches: number
    gen: number
    /** The anchor gen the client replayed — the host echoes its anchor only for
     *  THIS generation (never remap against a snapshot the client didn't replay). */
    clientAnchorGen?: number
  },
  signal: AbortSignal
) => Promise<RemoteTerminalSearchResult | null>

// Remote sources release the palette slot well before any transport default.
const REMOTE_FANOUT_TIMEOUT_MS = 8_000

/** Remap one host match into a client-space federated match, or null when it is
 *  not in the replayed window (deeper history / anchor mismatch) — those are
 *  counted as inline-only and surfaced via the batch's incomplete flag. */
function remapMatch(
  match: RemoteTerminalSearchResult['matches'][number],
  result: RemoteTerminalSearchResult,
  pane: DiscoveredRemotePane
): FederatedMatch | null {
  const responseAnchor =
    typeof result.hostRowAnchor === 'number' && typeof result.anchorGen === 'number'
      ? {
          hostRowAnchor: result.hostRowAnchor,
          anchorGen: result.anchorGen,
          // The SNAPSHOT-time host width (not the live hostCols) — stable across
          // a host resize, so a resize can never move where a stable row lands.
          anchorHostCols: result.anchorHostCols
        }
      : null
  const remap = remapRemoteSearchRow({
    matchHostRow: match.hostRow,
    responseAnchor,
    replayedAnchor: pane.replayedAnchor,
    replayOriginRow: pane.replayOriginRow,
    replayedRowCount: pane.replayedRowCount,
    clientCols: pane.clientCols ?? undefined
  })
  if (remap.kind !== 'in-window') {
    return null // out-of-window / anchor-mismatch → deeper-history inline expansion
  }
  return {
    absRow: remap.clientRow,
    col: match.col,
    len: match.len,
    snippet: match.line,
    ...(remap.approximate ? { approximate: true } : {})
  }
}

export function createRemotePaneSearchAdapter(deps: {
  discoverRemotePanes: () => DiscoveredRemotePane[]
  searchRemote: RemoteSearchCall
  timeoutMs?: number
}): SearchSourceAdapter {
  // One AbortController per in-flight generation so cancel() reaches the host.
  const controllers = new Map<number, AbortController>()

  const query: SearchSourceAdapter['query'] = async (q, opts, gen, maxPerPane, emit) => {
    const controller = new AbortController()
    controllers.set(gen, controller)
    const timer = setTimeout(() => controller.abort(), deps.timeoutMs ?? REMOTE_FANOUT_TIMEOUT_MS)
    try {
      // Fan out to every remote pane concurrently — one slow host never blocks
      // another (or the local sources merged by the controller).
      await Promise.all(
        deps.discoverRemotePanes().map((pane) =>
          searchOnePane(pane, q, opts, gen, maxPerPane, emit, deps.searchRemote, controller.signal)
        )
      )
    } finally {
      clearTimeout(timer)
      controllers.delete(gen)
    }
  }

  return {
    query,
    cancel: (gen) => {
      controllers.get(gen)?.abort()
      controllers.delete(gen)
    }
  }
}

async function searchOnePane(
  pane: DiscoveredRemotePane,
  q: string,
  opts: FederatedQueryOpts,
  gen: number,
  maxPerPane: number,
  emit: (batch: FederatedPaneBatch) => void,
  searchRemote: RemoteSearchCall,
  signal: AbortSignal
): Promise<void> {
  let result: RemoteTerminalSearchResult | null
  try {
    result = await searchRemote(
      pane,
      {
        query: q,
        caseSensitive: opts.caseSensitive,
        isRegex: opts.isRegex,
        maxMatches: maxPerPane,
        gen,
        ...(pane.replayedAnchor ? { clientAnchorGen: pane.replayedAnchor.anchorGen } : {})
      },
      signal
    )
  } catch {
    return // transport failure → source absent for this fan-out, never throws
  }
  if (!result || !result.available || signal.aborted) {
    return
  }
  const inWindow: FederatedMatch[] = []
  let inlineOnly = 0
  for (const match of result.matches) {
    const mapped = remapMatch(match, result, pane)
    if (mapped) {
      inWindow.push(mapped)
    } else {
      inlineOnly++
    }
  }
  inWindow.sort((a, b) => b.absRow - a.absRow || b.col - a.col)
  emit({
    paneRef: pane.paneRef,
    sessionId: pane.sessionId,
    source: 'remote',
    matches: inWindow.slice(0, maxPerPane > 0 ? maxPerPane : FEDERATED_TOP_K_MATCHES),
    total: result.total,
    // Honest: matches deeper than the replayed window (inline-only) or a host
    // truncation both mean "not everything is directly jumpable here".
    incomplete: result.incomplete || inlineOnly > 0,
    approxTime: pane.lastOutputAt || null
  })
}
