const liveClaudePtyIds = new Set<string>()
// Why: ids restored from persistence at startup, not yet confirmed against the
// daemon. They keep the OAuth refresh gate closed so an early managed refresh
// cannot rotate the single-use refresh token out from under a Claude CLI that
// survived the app restart inside the daemon.
const seededUnconfirmedPtyIds = new Set<string>()
let switchInProgress = false

export type ClaudeLivePtyPersistence = {
  addClaudeLivePtySessionId(sessionId: string): void
  removeClaudeLivePtySessionId(sessionId: string): void
}

let persistence: ClaudeLivePtyPersistence | null = null

export function attachClaudeLivePtyPersistence(target: ClaudeLivePtyPersistence | null): void {
  persistence = target
}

// Why: managed OAuth refresh is deferred while a Claude PTY is live (runtime-auth-service).
// When the last live Claude tab closes, the deferred refresh would otherwise wait for the
// next window-focus-gated poll (issue #9324). This drain listener fires on the true 1→0
// transition so the rate-limits service can run the deferred refresh immediately.
let drainListener: (() => void) | null = null

export function attachClaudeLivePtyDrainListener(listener: (() => void) | null): void {
  drainListener = listener
}

// Run `mutate`, then fire the drain listener only when the live set went from non-empty to empty.
function withLastPtyDrainNotification(mutate: () => void): void {
  const hadLivePtys = liveClaudePtyIds.size > 0
  mutate()
  if (hadLivePtys && liveClaudePtyIds.size === 0) {
    drainListener?.()
  }
}

export function seedLiveClaudePtysFromPersistence(sessionIds: readonly string[]): void {
  for (const sessionId of sessionIds) {
    liveClaudePtyIds.add(sessionId)
    seededUnconfirmedPtyIds.add(sessionId)
  }
}

export function hasSeededUnconfirmedClaudePtys(): boolean {
  return seededUnconfirmedPtyIds.size > 0
}

/**
 * Reconcile seeded ids against the daemon's live session list. Seeded ids the
 * daemon no longer knows are dead — release them so they cannot defer OAuth
 * refresh forever. Seeded ids that are still alive stay in the gate even if
 * their pane never reattaches: that daemon process still owns the credentials.
 */
export function confirmSeededClaudeLivePtys(aliveSessionIds: readonly string[]): void {
  withLastPtyDrainNotification(() => {
    const alive = new Set(aliveSessionIds)
    for (const sessionId of seededUnconfirmedPtyIds) {
      if (!alive.has(sessionId)) {
        liveClaudePtyIds.delete(sessionId)
        persistence?.removeClaudeLivePtySessionId(sessionId)
      }
    }
    seededUnconfirmedPtyIds.clear()
  })
}

export function markClaudePtySpawned(ptyId: string): void {
  liveClaudePtyIds.add(ptyId)
  seededUnconfirmedPtyIds.delete(ptyId)
  persistence?.addClaudeLivePtySessionId(ptyId)
}

export function markClaudePtyExited(ptyId: string): void {
  withLastPtyDrainNotification(() => {
    liveClaudePtyIds.delete(ptyId)
    seededUnconfirmedPtyIds.delete(ptyId)
    persistence?.removeClaudeLivePtySessionId(ptyId)
  })
}

export function hasLiveClaudePtys(): boolean {
  return liveClaudePtyIds.size > 0
}

export function isLiveClaudePty(ptyId: string): boolean {
  return liveClaudePtyIds.has(ptyId)
}

export function beginClaudeAuthSwitch(): void {
  if (switchInProgress) {
    throw new Error('A Claude account switch is already in progress.')
  }
  switchInProgress = true
}

export function endClaudeAuthSwitch(): void {
  switchInProgress = false
}

export function isClaudeAuthSwitchInProgress(): boolean {
  return switchInProgress
}
