import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { getState } = vi.hoisted(() => ({ getState: vi.fn() }))
vi.mock('@/store', () => ({ useAppStore: { getState } }))

import {
  __clearSelfMoveRegistryForTests,
  __getSelfMoveRegistrySizeForTests,
  clearSelfMove,
  hasRecentSelfMove,
  recordSelfMove,
  SELF_MOVE_REMOTE_TTL_MS
} from './editor-path-move-inflight'
import { recordSelfMoveForOpenTabs } from './record-self-move-for-open-tabs'

describe('editor-path-move-inflight registry', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    __clearSelfMoveRegistryForTests()
  })
  afterEach(() => {
    vi.useRealTimers()
    __clearSelfMoveRegistryForTests()
  })

  it('tracks source and target roles independently per path', () => {
    recordSelfMove('source', '/repo/old.md')
    expect(hasRecentSelfMove('source', '/repo/old.md')).toBe(true)
    // Same path, other role, must not be considered stamped.
    expect(hasRecentSelfMove('target', '/repo/old.md')).toBe(false)
  })

  it('clears exactly the retracted role/path', () => {
    recordSelfMove('target', '/repo/new.md')
    clearSelfMove('target', '/repo/new.md')
    expect(hasRecentSelfMove('target', '/repo/new.md')).toBe(false)
  })

  it('expires a local stamp after its TTL', () => {
    recordSelfMove('target', '/repo/new.md')
    vi.advanceTimersByTime(760)
    expect(hasRecentSelfMove('target', '/repo/new.md')).toBe(false)
  })

  it('keeps a remote stamp alive past the local TTL', () => {
    recordSelfMove('target', '/repo/new.md', 'env-1', SELF_MOVE_REMOTE_TTL_MS)
    vi.advanceTimersByTime(1000)
    expect(hasRecentSelfMove('target', '/repo/new.md', 'env-1')).toBe(true)
  })

  it('scopes stamps by runtime owner', () => {
    recordSelfMove('target', '/repo/new.md', 'env-1')
    expect(hasRecentSelfMove('target', '/repo/new.md', 'env-1')).toBe(true)
    expect(hasRecentSelfMove('target', '/repo/new.md', null)).toBe(false)
  })
})

describe('recordSelfMoveForOpenTabs', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    __clearSelfMoveRegistryForTests()
    getState.mockReset()
  })
  afterEach(() => {
    vi.useRealTimers()
    __clearSelfMoveRegistryForTests()
  })

  it('stamps source+target for every open tab under a directory move, and nothing else', () => {
    getState.mockReturnValue({
      openFiles: [
        { id: 'a', filePath: '/repo/dir/a.md', runtimeEnvironmentId: null },
        { id: 'b', filePath: '/repo/dir/sub/b.md', runtimeEnvironmentId: null },
        { id: 'c', filePath: '/repo/other/c.md', runtimeEnvironmentId: null }
      ]
    })

    recordSelfMoveForOpenTabs({ fromPath: '/repo/dir', toPath: '/repo/moved' })

    expect(hasRecentSelfMove('source', '/repo/dir/a.md')).toBe(true)
    expect(hasRecentSelfMove('target', '/repo/moved/a.md')).toBe(true)
    expect(hasRecentSelfMove('source', '/repo/dir/sub/b.md')).toBe(true)
    expect(hasRecentSelfMove('target', '/repo/moved/sub/b.md')).toBe(true)
    // The tab outside the moved directory is never stamped.
    expect(hasRecentSelfMove('source', '/repo/other/c.md')).toBe(false)
  })

  it('retraction clears every stamp it placed (failed move must not swallow real events)', () => {
    getState.mockReturnValue({
      openFiles: [{ id: 'a', filePath: '/repo/a.md', runtimeEnvironmentId: null }]
    })

    const retract = recordSelfMoveForOpenTabs({ fromPath: '/repo/a.md', toPath: '/repo/b.md' })
    expect(hasRecentSelfMove('target', '/repo/b.md')).toBe(true)

    retract()
    expect(hasRecentSelfMove('source', '/repo/a.md')).toBe(false)
    expect(hasRecentSelfMove('target', '/repo/b.md')).toBe(false)
    expect(__getSelfMoveRegistrySizeForTests()).toBe(0)
  })

  it('uses the remote TTL when the worktree has an SSH connection', () => {
    getState.mockReturnValue({
      openFiles: [{ id: 'a', filePath: '/repo/a.md', runtimeEnvironmentId: null }]
    })

    recordSelfMoveForOpenTabs({
      fromPath: '/repo/a.md',
      toPath: '/repo/b.md',
      connectionId: 'ssh-1'
    })
    // Past the local (750ms) window but within the remote window.
    vi.advanceTimersByTime(1000)
    expect(hasRecentSelfMove('target', '/repo/b.md')).toBe(true)
  })
})
