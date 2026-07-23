// Batch → group merging and ranking (FEDERATED-SEARCH-DESIGN §1 grouping):
// grouped by pane, merged on sessionId FIRST (a live pane and its daemon depth
// extension form ONE group, never two), matches newest-first (highest absRow),
// pane order: focused → visible → recency of last output → dead daemon sessions.

import {
  FEDERATED_TOP_K_MATCHES,
  type FederatedMatch,
  type FederatedPaneBatch,
  type FederatedPaneRef,
  type FederatedResultSource
} from './federated-search-model'
import { filterDepthExtensionMatches } from './federated-session-dedup'

export type FederatedResultGroup = {
  /** sessionId when daemon-backed (source-keyed identity), else the paneKey. */
  key: string
  paneRef?: FederatedPaneRef
  sessionId: string | null
  source: FederatedResultSource
  /** Newest-first, capped at FEDERATED_TOP_K_MATCHES. */
  matches: FederatedMatch[]
  /** Uncapped total across merged batches. */
  total: number
  incomplete: boolean
  /** §4 stale-skip / cost-gate marker ("results stale — output streaming"). */
  stale: boolean
  /** True once a daemon depth-extension batch merged in (badged as history depth). */
  hasDepthExtension: boolean
  /** §4 admission control refused to index this pane (honest empty group). */
  overBudget: boolean
  approxTime: number | null
}

/** Source-keyed group identity (§1): sessionId first so live + daemon merge. */
export function federatedGroupKey(
  batch: Pick<FederatedPaneBatch, 'sessionId' | 'paneRef'>
): string {
  if (batch.sessionId !== null) {
    return `session:${batch.sessionId}`
  }
  return `pane:${batch.paneRef?.paneKey ?? 'unresolved'}`
}

function sortNewestFirst(matches: FederatedMatch[]): FederatedMatch[] {
  return matches.sort((a, b) => b.absRow - a.absRow || b.col - a.col)
}

/** Merge one batch into the group map (mutating), deduping identical spans —
 *  the depth cutoff already prevents live/daemon overlap, but a skewed source
 *  must still never duplicate a row the live scan reported. */
export function mergeFederatedBatch(
  groups: Map<string, FederatedResultGroup>,
  batch: FederatedPaneBatch,
  /** Depth-extension cutoff for this batch's session, when one applies. */
  cutoffRow?: number
): void {
  const key = federatedGroupKey(batch)
  const incoming =
    batch.depthExtension && cutoffRow !== undefined
      ? filterDepthExtensionMatches(batch.matches, cutoffRow)
      : [...batch.matches]
  const existing = groups.get(key)
  if (!existing) {
    groups.set(key, {
      key,
      paneRef: batch.paneRef,
      sessionId: batch.sessionId,
      source: batch.source,
      matches: sortNewestFirst(incoming).slice(0, FEDERATED_TOP_K_MATCHES),
      total: batch.depthExtension ? incoming.length : batch.total,
      incomplete: batch.incomplete,
      stale: false,
      hasDepthExtension: batch.depthExtension === true,
      overBudget: batch.degraded === 'over-budget',
      approxTime: batch.approxTime
    })
    return
  }
  // A live batch supplies the pane identity/source; a depth extension never
  // overrides them (the pane stays the group's face).
  if (batch.paneRef && !existing.paneRef) {
    existing.paneRef = batch.paneRef
  }
  if (!batch.depthExtension) {
    existing.source = batch.source
  }
  const seen = new Set(existing.matches.map((m) => `${m.absRow}:${m.col}:${m.len}`))
  const fresh = incoming.filter((m) => !seen.has(`${m.absRow}:${m.col}:${m.len}`))
  existing.matches = sortNewestFirst([...existing.matches, ...fresh]).slice(
    0,
    FEDERATED_TOP_K_MATCHES
  )
  existing.total += batch.depthExtension ? fresh.length : batch.total
  existing.incomplete = existing.incomplete || batch.incomplete
  existing.hasDepthExtension = existing.hasDepthExtension || batch.depthExtension === true
  existing.overBudget = existing.overBudget || batch.degraded === 'over-budget'
  existing.approxTime = existing.approxTime ?? batch.approxTime
}

export type FederatedGroupOrderContext = {
  focusedPaneKey: string | null
  visiblePaneKeys: ReadonlySet<string>
  /** Higher = more recent output; unknown panes rank 0. */
  outputRecency: (paneKey: string) => number
}

/** §1 pane order: focused pane, visible panes, then recency of last output,
 *  then daemon-history entries for exited sessions. */
export function orderFederatedGroups(
  groups: Iterable<FederatedResultGroup>,
  ctx: FederatedGroupOrderContext
): FederatedResultGroup[] {
  const rank = (group: FederatedResultGroup): number => {
    const paneKey = group.paneRef?.paneKey
    if (!paneKey) {
      return 3 // dead daemon session — last
    }
    if (paneKey === ctx.focusedPaneKey) {
      return 0
    }
    return ctx.visiblePaneKeys.has(paneKey) ? 1 : 2
  }
  return [...groups].sort((a, b) => {
    const ra = rank(a)
    const rb = rank(b)
    if (ra !== rb) {
      return ra - rb
    }
    const recA = a.paneRef ? ctx.outputRecency(a.paneRef.paneKey) : (a.approxTime ?? 0)
    const recB = b.paneRef ? ctx.outputRecency(b.paneRef.paneKey) : (b.approxTime ?? 0)
    return recB - recA
  })
}
