// Jump-to-pane-and-row (FEDERATED-SEARCH-DESIGN §1 Navigation): live/hidden panes
// activate + scroll to the exact absolute row; PARKED panes un-park via normal
// activation, then the pinned query re-runs inside the restored engine and the
// jump lands on the match NEAREST the recorded absRow (row identity is not stable
// across a truncated replay), with a toast when the restored buffer no longer
// contains the match. Stale paneRefs degrade (§1 rev): daemon inline expansion
// when the sessionId persisted, a "pane no longer available" toast otherwise —
// never a throw, never a fallback pane.

import type { FederatedMatch, FederatedPaneRef, FederatedQueryOpts } from './federated-search-model'
import type { FederatedResultGroup } from './federated-search-grouping'

export type FederatedJumpOutcome =
  | 'jumped'
  | 'jumped-nearest'
  | 'missing-match'
  | 'daemon-inline'
  | 'pane-unavailable'

/** The pane surface a jump drives once the pane resolves. */
export type FederatedJumpPane = {
  scrollToLine: (absRow: number) => void
  focus: () => void
}

export type FederatedJumpDeps = {
  /** Resolve a live (mounted) pane, or null (closed / parked / remote-gone). */
  resolvePane: (paneRef: FederatedPaneRef) => FederatedJumpPane | null
  /** Activate the pane's worktree + tab (un-parks a parked pane). */
  activatePane: (paneRef: FederatedPaneRef) => void
  /** Await the pane's controller after activation (bounded); null on timeout. */
  waitForPane: (paneRef: FederatedPaneRef) => Promise<FederatedJumpPane | null>
  /** Re-run the pinned query inside the restored pane's engine (non-perturbing
   *  single-pane federated scan); null when the re-run failed/cancelled. */
  rerunQueryInPane: (
    paneRef: FederatedPaneRef,
    query: string,
    opts: FederatedQueryOpts
  ) => Promise<{ absRow: number }[] | null>
  /** Inline daemon context expansion for a persisted session; false = no daemon
   *  fallback available (adapter not wired / session gone). */
  expandDaemonInline: (sessionId: string, absRow: number) => boolean
  notifyJumpOutcome: (kind: 'missing-match' | 'pane-unavailable') => void
}

/** The match whose absRow is nearest the recorded one (ties → newer row). */
export function nearestFederatedMatchRow(
  rows: readonly { absRow: number }[],
  recordedAbsRow: number
): number | null {
  let best: number | null = null
  let bestDistance = Infinity
  for (const { absRow } of rows) {
    const distance = Math.abs(absRow - recordedAbsRow)
    if (distance < bestDistance || (distance === bestDistance && absRow > (best ?? -1))) {
      best = absRow
      bestDistance = distance
    }
  }
  return best
}

export async function jumpToFederatedResult(
  group: Pick<FederatedResultGroup, 'paneRef' | 'sessionId'>,
  match: FederatedMatch,
  query: string,
  opts: FederatedQueryOpts,
  deps: FederatedJumpDeps
): Promise<FederatedJumpOutcome> {
  const degradeToDaemonOrToast = (): FederatedJumpOutcome => {
    if (group.sessionId !== null && deps.expandDaemonInline(group.sessionId, match.absRow)) {
      return 'daemon-inline'
    }
    deps.notifyJumpOutcome('pane-unavailable')
    return 'pane-unavailable'
  }

  // Dead daemon session (no paneRef): inline expansion is the only navigation.
  if (!group.paneRef) {
    return degradeToDaemonOrToast()
  }
  const paneRef = group.paneRef

  const live = deps.resolvePane(paneRef)
  if (live) {
    // Live/hidden pane: exact in-engine jump (rows are the engine's own).
    deps.activatePane(paneRef)
    live.focus()
    live.scrollToLine(match.absRow)
    return 'jumped'
  }

  // Parked (or freshly closed): activation un-parks; the engine restores from
  // snapshot/replay, so re-run-and-jump-nearest is the contract.
  deps.activatePane(paneRef)
  const restored = await deps.waitForPane(paneRef)
  if (!restored) {
    return degradeToDaemonOrToast()
  }
  const rows = await deps.rerunQueryInPane(paneRef, query, opts)
  const nearest = rows && rows.length > 0 ? nearestFederatedMatchRow(rows, match.absRow) : null
  restored.focus()
  if (nearest === null) {
    // The truncated replay no longer contains the match — say so, don't guess.
    deps.notifyJumpOutcome('missing-match')
    return 'missing-match'
  }
  restored.scrollToLine(nearest)
  return 'jumped-nearest'
}
