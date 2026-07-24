import type { Worktree } from './workspace-list-types'

/**
 * Reconcile local pin state against a worktree.ps snapshot.
 *
 * Mutates `pending` (worktreeId -> desired isPinned) in place: an override is dropped once the
 * snapshot confirms the desired state, or the worktree vanishes from the snapshot. Until then the
 * override wins, so a poll that predates the desktop applying worktree.set can't revert (or persist
 * away) an optimistic pin.
 *
 * Returns null when the snapshot omits isPinned entirely (older desktop). The caller must then skip
 * overwriting/persisting local pins — treating absence as "all unpinned" would wipe pins every poll.
 */
export function reconcilePinnedIds(
  worktrees: Worktree[],
  pending: Map<string, boolean>
): Set<string> | null {
  if (!worktrees.some((w) => 'isPinned' in w)) {
    return null
  }
  const serverPinned = new Set(worktrees.filter((w) => w.isPinned).map((w) => w.worktreeId))
  if (pending.size === 0) {
    return serverPinned
  }
  const known = new Set(worktrees.map((w) => w.worktreeId))
  // Deleting the current entry mid-iteration is safe per the Map iterator spec.
  for (const [id, desired] of pending) {
    if (!known.has(id)) {
      pending.delete(id) // worktree vanished from the snapshot; drop the stale override
    } else if (serverPinned.has(id) === desired) {
      pending.delete(id) // server confirmed the desired state
    } else if (desired) {
      serverPinned.add(id)
    } else {
      serverPinned.delete(id)
    }
  }
  return serverPinned
}

/**
 * Handle a failed / non-ok worktree.set for an optimistic pin toggle. Drops the pending override so
 * the next poll reconciles the local pin back to server truth. Without this, reconcilePinnedIds only
 * clears an override on confirmation, so a set that never applies would re-force the unconfirmed
 * value into pinnedIds every poll and savePinnedIds would re-persist it — a permanent desync.
 *
 * Returns true when this request still owns the override (caller should roll local pin state back to
 * the pre-toggle value); false when a newer toggle superseded it (that request now owns the state,
 * so leave both the override and the local pin alone).
 */
export function revertPendingPin(
  pending: Map<string, boolean>,
  worktreeId: string,
  attempted: boolean
): boolean {
  if (pending.get(worktreeId) !== attempted) {
    return false
  }
  pending.delete(worktreeId)
  return true
}
