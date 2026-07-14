import type { Repo } from './types'

export const UNGROUPED_PROJECT_GROUP_KEY = 'project-group:ungrouped'

// normalizeProjectGroupName, createProjectGroup, normalizeProjectGroups, and
// clearMissingProjectGroupMemberships were cut over to the Rust project-groups
// core (orca-config::project_groups). They're main-process-only (persistence.ts)
// so they run through napi with no fallback window; see
// src/main/rust-project-groups.ts. getProjectGroupSubtreeIds +
// getNextProjectGroupOrder went the same way earlier. Only the two below stay in
// TS: getEffectiveProjectGroupManualRank is a render-hot sort comparator that
// returns Infinity and reads a Map (a JSON round-trip per comparison is the
// wrong boundary), and UNGROUPED_PROJECT_GROUP_KEY is a module const.

/** Manual rank for a project inside a group bucket. Explicit
 *  `projectGroupOrder` wins; otherwise fall back to global repo order so drag
 *  midpoint math and sidebar sorting stay aligned. */
export function getEffectiveProjectGroupManualRank(
  repo: Pick<Repo, 'id' | 'projectGroupOrder'> | undefined,
  repoOrderRankById?: ReadonlyMap<string, number>,
  siblingFallbackIndex?: number
): number {
  if (!repo) {
    return Number.POSITIVE_INFINITY
  }
  const order = repo.projectGroupOrder
  if (typeof order === 'number' && Number.isFinite(order)) {
    return order
  }
  const repoRank = repoOrderRankById?.get(repo.id)
  if (repoRank !== undefined) {
    return repoRank * 1000
  }
  if (siblingFallbackIndex !== undefined) {
    return siblingFallbackIndex * 1000
  }
  return Number.POSITIVE_INFINITY
}
