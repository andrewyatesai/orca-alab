import { describe, it, expect } from 'vitest'
import { join } from 'node:path'
import { reconcilePinnedIds, revertPendingPin } from './pin-reconciliation'
import type { Worktree } from './workspace-list-types'

function worktree(overrides: Partial<Worktree> = {}): Worktree {
  const worktreePath = join('/tmp', 'orca', 'worktrees', overrides.worktreeId ?? 'manta')
  return {
    worktreeId: overrides.worktreeId ?? 'w1',
    repoId: 'repo-1',
    repo: 'orca',
    branch: 'feature/x',
    displayName: 'manta',
    workspaceStatus: 'in-progress',
    path: worktreePath,
    liveTerminalCount: 0,
    hasAttachedPty: false,
    preview: '',
    unread: false,
    isPinned: false,
    isActive: false,
    linkedPR: null,
    ...overrides
  }
}

describe('reconcilePinnedIds', () => {
  it('mirrors server pins when there is no pending optimistic toggle', () => {
    const pending = new Map<string, boolean>()
    const result = reconcilePinnedIds(
      [
        worktree({ worktreeId: 'a', isPinned: true }),
        worktree({ worktreeId: 'b', isPinned: false })
      ],
      pending
    )
    expect(result).toEqual(new Set(['a']))
    expect(pending.size).toBe(0)
  })

  it('returns null (skips overwrite) when the snapshot omits isPinned entirely (older desktop)', () => {
    // Older desktops send rows without the isPinned field; typed required but absent at runtime.
    const row = worktree({ worktreeId: 'a' })
    delete (row as Partial<Worktree>).isPinned
    const pending = new Map<string, boolean>()
    expect(reconcilePinnedIds([row], pending)).toBeNull()
  })

  it('treats an explicit isPinned:false snapshot as authoritative, not as absent', () => {
    const result = reconcilePinnedIds([worktree({ worktreeId: 'a', isPinned: false })], new Map())
    expect(result).toEqual(new Set())
  })

  it('keeps an optimistic pin when the snapshot predates the desktop applying worktree.set', () => {
    // User just pinned 'a'; the racing poll's snapshot still shows it unpinned.
    const pending = new Map<string, boolean>([['a', true]])
    const result = reconcilePinnedIds([worktree({ worktreeId: 'a', isPinned: false })], pending)
    expect(result).toEqual(new Set(['a']))
    // Override retained until the server confirms.
    expect(pending.get('a')).toBe(true)
  })

  it('keeps an optimistic unpin when the snapshot still shows it pinned', () => {
    const pending = new Map<string, boolean>([['a', false]])
    const result = reconcilePinnedIds([worktree({ worktreeId: 'a', isPinned: true })], pending)
    expect(result).toEqual(new Set())
    expect(pending.get('a')).toBe(false)
  })

  it('clears the override once the server confirms the desired pin', () => {
    const pending = new Map<string, boolean>([['a', true]])
    const result = reconcilePinnedIds([worktree({ worktreeId: 'a', isPinned: true })], pending)
    expect(result).toEqual(new Set(['a']))
    expect(pending.has('a')).toBe(false)
  })

  it('is idempotent across a double invocation (safe inside a React state updater)', () => {
    // The reconciler runs inside setPinnedIds, which React may invoke twice; the result and the
    // pruned pending map must be identical whether it runs once or twice.
    const rows = [
      worktree({ worktreeId: 'a', isPinned: true }),
      worktree({ worktreeId: 'b', isPinned: false })
    ]
    const pending = new Map<string, boolean>([
      ['a', true], // confirmed -> dropped on first pass
      ['b', true] // still optimistic -> retained
    ])
    const first = reconcilePinnedIds(rows, pending)
    const pendingAfterFirst = new Map(pending)
    const second = reconcilePinnedIds(rows, pending)
    expect(second).toEqual(first)
    expect(second).toEqual(new Set(['a', 'b']))
    expect([...pending]).toEqual([...pendingAfterFirst])
    expect(pending.has('a')).toBe(false)
    expect(pending.get('b')).toBe(true)
  })

  it('drops a stale override when its worktree vanishes from the snapshot', () => {
    const pending = new Map<string, boolean>([['gone', true]])
    const result = reconcilePinnedIds([worktree({ worktreeId: 'a', isPinned: false })], pending)
    expect(result).toEqual(new Set())
    expect(pending.has('gone')).toBe(false)
  })
})

describe('revertPendingPin', () => {
  it('clears the override and signals rollback after a failed/non-ok worktree.set', () => {
    // A set that never applies must not leave the override stuck forever (reconcilePinnedIds would
    // otherwise re-force the optimistic value into pinnedIds every poll and re-persist it).
    const pending = new Map<string, boolean>([['a', true]])
    expect(revertPendingPin(pending, 'a', true)).toBe(true)
    expect(pending.has('a')).toBe(false)
    // Next poll now reconciles the local pin back to server truth (server still shows unpinned).
    expect(reconcilePinnedIds([worktree({ worktreeId: 'a', isPinned: false })], pending)).toEqual(
      new Set()
    )
  })

  it('leaves a newer toggle untouched when the failure belongs to a superseded request', () => {
    // User pinned 'a' (attempt=true), then un-pinned it before the first set failed (pending=false).
    const pending = new Map<string, boolean>([['a', false]])
    expect(revertPendingPin(pending, 'a', true)).toBe(false)
    // The newer optimistic un-pin still owns the state.
    expect(pending.get('a')).toBe(false)
  })

  it('is a no-op when there is no pending override for the worktree', () => {
    const pending = new Map<string, boolean>()
    expect(revertPendingPin(pending, 'a', true)).toBe(false)
    expect(pending.size).toBe(0)
  })
})
