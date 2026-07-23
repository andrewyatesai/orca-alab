// Daemon → live row-space mapping for the depth-extension cutoff (fed §2.3, the
// (d) blocker). A daemon search returns matches in DAEMON coordinates: 0-based
// from the oldest retained history row. The live engine's cutoff (its oldest
// retained absolute row) is in LIVE engine absolute-row space. The two origins
// differ — the daemon keeps far MORE history (5 MB) than the live renderer
// window — so a depth-extension match cannot be compared to the cutoff, ordered
// newest-first against live matches, or deduped, until it is mapped into the
// live engine's absolute-row space.
//
// Alignment: the daemon and the live pane track the SAME PTY, so their NEWEST
// row is the same row. Daemon row (daemonRowCount − 1) ≡ liveNewestAbsRow. Every
// older daemon row maps by walking back from there:
//
//   liveRow(daemonRow) = liveNewestAbsRow − (daemonRowCount − 1 − daemonRow)
//
// The depth extension keeps only rows whose live-mapped row is STRICTLY BELOW
// the live window's oldest row (those the live scan already covered are dropped).

import type { FederatedMatch } from './federated-search-model'

export type DaemonToLiveRowMapping = {
  /** Rows the daemon retains for this session (its history + visible span). */
  daemonRowCount: number
  /** The live engine's NEWEST absolute row (shared with the daemon's newest). */
  liveNewestAbsRow: number
}

/** Map one daemon-space row into the live engine's absolute-row space. */
export function mapDaemonRowToLive(daemonRow: number, mapping: DaemonToLiveRowMapping): number {
  return mapping.liveNewestAbsRow - (mapping.daemonRowCount - 1 - daemonRow)
}

/** The daemon-space cutoff to request: the smallest daemon row that maps to (or
 *  past) `oldestLiveAbsRow`. The daemon returns rows STRICTLY BELOW this, i.e.
 *  exactly the rows older than the live window — so the live scan and the depth
 *  extension never both cover a row. Off-by-N boundary:
 *    daemonRow = cutoff − 1  → liveRow = oldestLiveAbsRow − 1  (included)
 *    daemonRow = cutoff      → liveRow = oldestLiveAbsRow      (excluded: live-covered) */
export function daemonDepthCutoffRow(
  mapping: DaemonToLiveRowMapping,
  oldestLiveAbsRow: number
): number {
  // Solve liveRow(cutoff) = oldestLiveAbsRow for cutoff.
  return oldestLiveAbsRow - mapping.liveNewestAbsRow + (mapping.daemonRowCount - 1)
}

/** Remap a daemon depth-extension batch's matches into live absolute-row space,
 *  dropping any that map at/above the live window's oldest row (defense in depth:
 *  the daemon was told the cutoff, but a skewed daemon must still never surface a
 *  row the live scan already reported). Snippets/col/len are unchanged — only the
 *  row coordinate is translated. */
export function mapDaemonDepthMatchesToLive(
  matches: readonly FederatedMatch[],
  mapping: DaemonToLiveRowMapping,
  oldestLiveAbsRow: number
): FederatedMatch[] {
  const out: FederatedMatch[] = []
  for (const match of matches) {
    const liveRow = mapDaemonRowToLive(match.absRow, mapping)
    if (liveRow < oldestLiveAbsRow) {
      out.push({ ...match, absRow: liveRow })
    }
  }
  return out
}
