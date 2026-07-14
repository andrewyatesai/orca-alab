// Renderer project-group subtree collection, driven by the Rust project-groups
// core in the orca-git wasm module (the shared TS impl was gutted). Only the
// pure subtree helper is used in the renderer; create/normalize/manual-rank stay
// in TS.
import type { ProjectGroup } from '../../../../shared/types'
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'

export function getProjectGroupSubtreeIds(
  groups: readonly Pick<ProjectGroup, 'id' | 'parentGroupId'>[],
  rootGroupId: string
): Set<string> {
  // Wasm-not-ready fallback: the root alone. Consumers use this set for
  // membership only (scoping removals/queries to a subtree); under-scoping to
  // the root can't wrongly delete/hide descendants, and it's rare — first paint
  // gates on wasm-ready. A ready result is the full parity-proven subtree.
  if (!isGitWasmReady()) {
    return new Set([rootGroupId])
  }
  const nodes = groups.map((group) => ({ id: group.id, parentGroupId: group.parentGroupId ?? null }))
  // The Rust op returns a sorted array (membership is all any caller needs).
  const ids = JSON.parse(
    orcaDispatch('project-groups', 'getProjectGroupSubtreeIds', JSON.stringify({ groups: nodes, rootGroupId }))
  ) as string[]
  return new Set(ids)
}
