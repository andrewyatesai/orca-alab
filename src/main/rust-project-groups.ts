// Main-process project-group helpers, driven by the Rust project-groups core
// via napi (the shared TS impls were gutted). One source of truth with the
// parity-proven Rust port. The group id (crypto RNG) and the clock (Date.now)
// are generated HERE — the IO edge owns non-determinism; Rust stays pure.
// Only getEffectiveProjectGroupManualRank + UNGROUPED_PROJECT_GROUP_KEY stay in
// TS (render-hot sort comparator / Infinity / module const — the JSON boundary
// is the wrong tool there).
import type { ProjectGroup, ProjectGroupCreatedFrom, Repo } from '../shared/types'
import { requireRustGitBinding } from './daemon/rust-git-addon'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('project-groups', fn, JSON.stringify(input ?? null))
  )
}

function createProjectGroupId(): string {
  const randomUUID = globalThis.crypto?.randomUUID
  if (randomUUID) {
    return randomUUID.call(globalThis.crypto)
  }
  return `project-group-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

export function normalizeProjectGroupName(name: string, fallback?: string): string {
  // Absent fallback → Rust applies the 'Untitled group' default parameter.
  return dispatch('normalizeProjectGroupName', { name, fallback }) as string
}

export function createProjectGroup(input: {
  name: string
  parentPath?: string | null
  connectionId?: string | null
  parentGroupId?: string | null
  createdFrom: ProjectGroupCreatedFrom
  tabOrder: number
  now?: number
}): ProjectGroup {
  return dispatch('createProjectGroup', {
    id: createProjectGroupId(),
    name: input.name,
    parentPath: input.parentPath ?? null,
    connectionId: input.connectionId ?? null,
    parentGroupId: input.parentGroupId ?? null,
    createdFrom: input.createdFrom,
    tabOrder: input.tabOrder,
    now: input.now ?? Date.now()
  }) as ProjectGroup
}

export function normalizeProjectGroups(value: unknown): ProjectGroup[] {
  return dispatch('normalizeProjectGroups', { value, now: Date.now() }) as ProjectGroup[]
}

export function clearMissingProjectGroupMemberships(repos: Repo[], groups: ProjectGroup[]): Repo[] {
  // Full repo objects go over the wire so every field survives (the op only
  // nulls a dead projectGroupId); Rust reads just group ids.
  return dispatch('clearMissingProjectGroupMemberships', {
    repos,
    groups: groups.map((group) => ({ id: group.id }))
  }) as Repo[]
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
