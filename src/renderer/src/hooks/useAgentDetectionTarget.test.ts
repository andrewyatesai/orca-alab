import { describe, expect, it } from 'vitest'
import {
  getAgentDetectionTargetKeyForWorktree,
  parseAgentDetectionTargetKey
} from './useAgentDetectionTarget'

type OwnerState = Parameters<typeof getAgentDetectionTargetKeyForWorktree>[0]

function makeState(overrides: Record<string, unknown>): OwnerState {
  return {
    settings: { activeRuntimeEnvironmentId: null },
    folderWorkspaces: [],
    projectGroups: [],
    repos: [],
    worktreesByRepo: {},
    ...overrides
  } as unknown as OwnerState
}

describe('getAgentDetectionTargetKeyForWorktree', () => {
  it('routes an SSH-owned worktree to its connection host', () => {
    const state = makeState({
      repos: [{ id: 'repo-1', connectionId: 'ssh-1' }],
      worktreesByRepo: { 'repo-1': [{ id: 'repo-1::wt', repoId: 'repo-1' }] }
    })

    expect(getAgentDetectionTargetKeyForWorktree(state, 'repo-1::wt')).toBe('ssh:ssh-1')
  })

  it('routes a paired-runtime worktree to its runtime host, not the local client (#9790)', () => {
    // Repro for the "Remote Server lists local agents" bug: a worktree owned by
    // a paired runtime must resolve to that runtime, never fall back to local.
    const state = makeState({
      repos: [{ id: 'repo-1', connectionId: null }],
      worktreesByRepo: {
        'repo-1': [{ id: 'repo-1::wt', repoId: 'repo-1', hostId: 'runtime:env-1' }]
      }
    })

    expect(getAgentDetectionTargetKeyForWorktree(state, 'repo-1::wt')).toBe('runtime:env-1')
  })

  it('routes a plain local worktree to the local host', () => {
    const state = makeState({
      repos: [{ id: 'repo-1', connectionId: null }],
      worktreesByRepo: { 'repo-1': [{ id: 'repo-1::wt', repoId: 'repo-1' }] }
    })

    expect(getAgentDetectionTargetKeyForWorktree(state, 'repo-1::wt')).toBe('local')
  })

  it('stays unresolved while the owning repo has not hydrated', () => {
    expect(getAgentDetectionTargetKeyForWorktree(makeState({}), 'repo-1::wt')).toBeUndefined()
  })
})

describe('parseAgentDetectionTargetKey', () => {
  it('maps each key form back to a detection target', () => {
    expect(parseAgentDetectionTargetKey(undefined)).toBeUndefined()
    expect(parseAgentDetectionTargetKey('local')).toEqual({ kind: 'local' })
    expect(parseAgentDetectionTargetKey('ssh:ssh-1')).toEqual({
      kind: 'ssh',
      connectionId: 'ssh-1'
    })
    expect(parseAgentDetectionTargetKey('runtime:env-1')).toEqual({
      kind: 'runtime',
      environmentId: 'env-1'
    })
  })
})
