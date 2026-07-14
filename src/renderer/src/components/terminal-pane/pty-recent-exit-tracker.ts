// Exit code of PTYs that have already exited, so a watcher that calls
// subscribeToPtyExit AFTER the exit fired (and was consumed by the primary
// handler + the sidecar set deleted) is still notified — otherwise
// observeExistingAutomationSession can miss an exit in the subscribe window and
// leave the run hung. PTY ids are minted unique and never reused, so subscribing
// to an exited id always means "watch that exited session" → replay is correct.
// Bounded: cap + evict oldest (exits accumulate one per session over app life).
const recentPtyExits = new Map<string, number>()
const RECENT_PTY_EXIT_MAX = 256

export function recordRecentPtyExit(ptyId: string, code: number): void {
  if (!recentPtyExits.has(ptyId) && recentPtyExits.size >= RECENT_PTY_EXIT_MAX) {
    const oldest = recentPtyExits.keys().next().value
    if (typeof oldest === 'string') {
      recentPtyExits.delete(oldest)
    }
  }
  recentPtyExits.set(ptyId, code)
}

export function getRecentPtyExit(ptyId: string): number | undefined {
  return recentPtyExits.get(ptyId)
}
