import { describe, expect, it } from 'vitest'
import { getEffectiveProjectGroupManualRank } from './project-groups'
// normalizeProjectGroupName, createProjectGroup, normalizeProjectGroups,
// clearMissingProjectGroupMemberships, getNextProjectGroupOrder, and
// getProjectGroupSubtreeIds were cut over to the Rust core — their cases live in
// the Rust twin's #[cfg(test)] + tools/parity/vectors/project-groups.json.
import type { Repo } from './types'

function repo(overrides: Partial<Repo>): Repo {
  return {
    id: overrides.id ?? 'repo-1',
    path: overrides.path ?? '/repo',
    displayName: overrides.displayName ?? 'repo',
    badgeColor: '#999',
    addedAt: 1,
    kind: 'git',
    ...overrides
  }
}

describe('project-groups', () => {
  it('falls back to global repo order when projectGroupOrder is unset', () => {
    const repoOrder = new Map([
      ['a', 0],
      ['b', 2]
    ])

    expect(
      getEffectiveProjectGroupManualRank(repo({ id: 'a', projectGroupOrder: 5 }), repoOrder)
    ).toBe(5)
    expect(getEffectiveProjectGroupManualRank(repo({ id: 'a' }), repoOrder)).toBe(0)
    expect(getEffectiveProjectGroupManualRank(repo({ id: 'b' }), repoOrder)).toBe(2000)
    expect(getEffectiveProjectGroupManualRank(repo({ id: 'c' }), repoOrder, 1)).toBe(1000)
  })
})
