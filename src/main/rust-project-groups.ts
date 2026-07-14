// Main-process project-group helpers, driven by the Rust project-groups core
// via napi (the shared TS impls were gutted). One source of truth with the
// parity-proven Rust port. Only the pure, struct-drift-free helpers are routed
// here; create/normalize/clear-membership stay in TS (they must preserve full
// ProjectGroup/Repo shapes the lean Rust structs don't model).
import type { ProjectGroup, Repo } from '../shared/types'
import { requireRustGitBinding } from './daemon/rust-git-addon'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('project-groups', fn, JSON.stringify(input ?? null))
  )
}

export function getProjectGroupSubtreeIds(
  groups: readonly Pick<ProjectGroup, 'id' | 'parentGroupId'>[],
  rootGroupId: string
): Set<string> {
  // The Rust op returns a sorted array (membership is all any caller needs);
  // rebuild the Set the original returned. Send only the two fields it reads.
  const nodes = groups.map((group) => ({ id: group.id, parentGroupId: group.parentGroupId ?? null }))
  return new Set(dispatch('getProjectGroupSubtreeIds', { groups: nodes, rootGroupId }) as string[])
}

export function getNextProjectGroupOrder(repos: readonly Repo[], groupId: string | null): number {
  return dispatch('getNextProjectGroupOrder', { repos, groupId }) as number
}
