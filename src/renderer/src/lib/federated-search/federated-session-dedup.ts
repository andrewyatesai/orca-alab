// The live↔daemon session-dedup plan (FEDERATED-SEARCH-DESIGN §2.3, binding):
// resolved BEFORE fan-out. Sessions attached to a live/hidden/parked pane are
// EXCLUDED from standalone daemon-history results (else every daemon-backed pane
// double-reports); attached sessions are still searched as DEPTH EXTENSIONS —
// only rows older than the live window's oldest row, merged into the live group
// under the same sessionId. Dead sessions surface standalone (paneRef absent).

import type { FederatedMatch } from './federated-search-model'

export type FederatedDedupPane = {
  paneKey: string
  /** Daemon session backing this pane, or null when not daemon-backed. */
  sessionId: string | null
  /** Oldest absolute row the live engine still retains (the depth-extension
   *  cutoff); null when unknown — then the session gets NO depth extension
   *  (never risk double-reporting rows the live scan already covered). */
  oldestLiveRow: number | null
}

export type FederatedDepthExtension = {
  sessionId: string
  paneKey: string
  /** Daemon returns only matches at rows STRICTLY BELOW this row. */
  cutoffRow: number
}

export type FederatedDedupPlan = {
  /** The `searchSessions` allowlist: sessions with NO attached pane. */
  standaloneSessionIds: string[]
  /** Attached sessions searched only below their cutoff. */
  depthExtensions: FederatedDepthExtension[]
}

/** Compute the dedup plan from the attached-pane snapshot + the daemon's known
 *  sessions. Attached = any pane currently claiming the sessionId (live, hidden,
 *  or parked — a parked pane's snapshot search covers its recent window). */
export function planFederatedSessionDedup(
  panes: readonly FederatedDedupPane[],
  daemonSessionIds: readonly string[]
): FederatedDedupPlan {
  const attached = new Map<string, FederatedDedupPane>()
  for (const pane of panes) {
    if (pane.sessionId !== null && !attached.has(pane.sessionId)) {
      attached.set(pane.sessionId, pane)
    }
  }
  const standaloneSessionIds: string[] = []
  const depthExtensions: FederatedDepthExtension[] = []
  for (const sessionId of daemonSessionIds) {
    const pane = attached.get(sessionId)
    if (!pane) {
      standaloneSessionIds.push(sessionId)
      continue
    }
    if (pane.oldestLiveRow !== null) {
      depthExtensions.push({ sessionId, paneKey: pane.paneKey, cutoffRow: pane.oldestLiveRow })
    }
  }
  return { standaloneSessionIds, depthExtensions }
}

/** Enforce the cutoff on a depth-extension batch (defense in depth: the daemon
 *  is told the cutoff, but a skewed daemon must still never double-report). */
export function filterDepthExtensionMatches(
  matches: readonly FederatedMatch[],
  cutoffRow: number
): FederatedMatch[] {
  return matches.filter((m) => m.absRow < cutoffRow)
}
