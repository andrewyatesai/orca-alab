import { normalizeAbsolutePathForComparison } from '@/components/right-sidebar/file-explorer-paths'

// Why: an in-app move/rename re-homes the open tab to the new path (carrying its
// draft) and then physically relocates the file, which the worktree watcher
// reports as delete(old)+create(new) a few ms later. Because the tab already
// lives at the new path, that echo looks like an external write landing on a
// dirty tab and raises a spurious "changed on disk" banner (#9506). This is the
// move analog of editor-self-write-registry: stamp the source+target paths
// right before the on-disk rename so the watch hook recognizes the move's own
// echo and suppresses it, bounded by a short TTL so a genuinely external edit
// after the window still surfaces. A move carries no bytes to echo-verify the
// way a self-write does, so suppression within the TTL is a documented,
// bounded trade-off (the draft is preserved regardless of the banner).
const SELF_MOVE_TTL_MS = 750
// Why: SSH/runtime watcher echoes travel a poll-plus-network path and can land
// seconds after the rename, so remote moves need a wider window.
export const SELF_MOVE_REMOTE_TTL_MS = 3000
// Why: cap above realistic bulk directory-move sizes so a large move never
// self-evicts its own not-yet-echoed stamps.
const SELF_MOVE_MAX_STAMPS = 8192

export type SelfMoveRole = 'source' | 'target'

type SelfMoveStamp = {
  expiresAt: number
}

const stamps = new Map<string, SelfMoveStamp>()

function selfMoveKey(
  role: SelfMoveRole,
  absolutePath: string,
  runtimeEnvironmentId?: string | null
): string {
  return `${role}::${runtimeEnvironmentId?.trim() || 'client'}::${normalizeAbsolutePathForComparison(absolutePath)}`
}

function pruneExpiredSelfMoves(now = Date.now()): void {
  for (const [key, stamp] of stamps) {
    if (now > stamp.expiresAt) {
      stamps.delete(key)
    }
  }
}

function enforceSelfMoveStampLimit(): void {
  while (stamps.size > SELF_MOVE_MAX_STAMPS) {
    const oldest = stamps.keys().next().value
    if (oldest === undefined) {
      break
    }
    stamps.delete(oldest)
  }
}

export function recordSelfMove(
  role: SelfMoveRole,
  absolutePath: string,
  runtimeEnvironmentId?: string | null,
  ttlMs: number = SELF_MOVE_TTL_MS
): void {
  const now = Date.now()
  pruneExpiredSelfMoves(now)
  const key = selfMoveKey(role, absolutePath, runtimeEnvironmentId)
  // Why: a missing watcher echo should not leave a stale stamp resident for the
  // whole renderer session; re-stamping refreshes the window from the write.
  stamps.delete(key)
  stamps.set(key, { expiresAt: now + ttlMs })
  enforceSelfMoveStampLimit()
}

export function clearSelfMove(
  role: SelfMoveRole,
  absolutePath: string,
  runtimeEnvironmentId?: string | null
): void {
  stamps.delete(selfMoveKey(role, absolutePath, runtimeEnvironmentId))
}

export function hasRecentSelfMove(
  role: SelfMoveRole,
  absolutePath: string,
  runtimeEnvironmentId?: string | null
): boolean {
  const key = selfMoveKey(role, absolutePath, runtimeEnvironmentId)
  const stamp = stamps.get(key)
  if (!stamp) {
    return false
  }
  if (Date.now() > stamp.expiresAt) {
    stamps.delete(key)
    return false
  }
  return true
}

export function __clearSelfMoveRegistryForTests(): void {
  stamps.clear()
}

export function __getSelfMoveRegistrySizeForTests(): number {
  return stamps.size
}
