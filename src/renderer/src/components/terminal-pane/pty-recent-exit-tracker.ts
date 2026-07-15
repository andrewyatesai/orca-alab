// Exit code of PTYs that have already exited, so a watcher that calls
// subscribeToPtyExit AFTER the exit fired (and was consumed by the primary
// handler + the sidecar set deleted) is still notified — otherwise
// observeExistingAutomationSession can miss an exit in the subscribe window and
// leave the run hung. PTY ids are minted unique and never reused, so subscribing
// to an exited id always means "watch that exited session" → replay is correct.
// Bounded: cap + evict oldest (exits accumulate one per session over app life).
// hadPrimary carries whether a pane transport owned the exit, so a replay-on-
// subscribe sidecar (e.g. parked-tab teardown) can branch identically to a live one.
export type RecentPtyExit = { code: number; hadPrimary: boolean }
const recentPtyExits = new Map<string, RecentPtyExit>()
const RECENT_PTY_EXIT_MAX = 256

export function recordRecentPtyExit(ptyId: string, code: number, hadPrimary: boolean): void {
  if (!recentPtyExits.has(ptyId) && recentPtyExits.size >= RECENT_PTY_EXIT_MAX) {
    const oldest = recentPtyExits.keys().next().value
    if (typeof oldest === 'string') {
      recentPtyExits.delete(oldest)
    }
  }
  recentPtyExits.set(ptyId, { code, hadPrimary })
}

export function getRecentPtyExit(ptyId: string): RecentPtyExit | undefined {
  return recentPtyExits.get(ptyId)
}
