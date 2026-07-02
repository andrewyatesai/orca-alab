/** Coalesces canvas redraws into one frame: a single rAF plus a setTimeout
 *  backstop (rAF is paused for hidden/occluded windows, so the timer guarantees
 *  the draw still lands for background panes + headless e2e). The two race; the
 *  winner clears the loser so the backstop never fires stale and timers never
 *  pile up. dispose() cancels any pending frame so nothing runs after teardown. */
export type AtermDrawScheduler = {
  /** Schedule a draw if one isn't already pending (idempotent within a frame). */
  schedule: () => void
  /** Mark the scheduled frame as consumed; call at the top of the draw body. */
  consume: () => void
  /** True once a draw is scheduled and not yet consumed. */
  isScheduled: () => boolean
  /** True while draw scheduling is paused for a hidden pane. */
  isSuspended: () => boolean
  /** Pause/resume draw scheduling for a hidden pane. While suspended, schedule()
   *  records that a draw is wanted but fires no rAF/timer; resume runs one draw
   *  if anything was scheduled while paused so the pane repaints its latest state. */
  setSuspended: (suspended: boolean) => void
  /** Cancel any pending rAF/timer (call on dispose). */
  dispose: () => void
}

// 33ms ≈ one 30fps frame: long enough that the rAF usually wins on a visible
// window, short enough that a hidden/occluded pane still paints promptly.
const BACKSTOP_TIMEOUT_MS = 33

export function createAtermDrawScheduler(runDraw: () => void): AtermDrawScheduler {
  let scheduled = false
  let suspended = false
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  let rafId: number | null = null

  const clearTimer = (): void => {
    if (timeoutId !== null) {
      clearTimeout(timeoutId)
      timeoutId = null
    }
  }

  // Keep the rAF id so dispose/suspend can really cancel the pending frame —
  // an uncancelled rAF would run the draw after teardown (a fired id is a no-op
  // to cancel, so clearing after the race is always safe).
  const clearRaf = (): void => {
    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
  }

  const arm = (): void => {
    clearRaf()
    rafId = requestAnimationFrame(runDraw)
    // Replace any prior backstop so timers never accumulate across schedules.
    clearTimer()
    timeoutId = setTimeout(runDraw, BACKSTOP_TIMEOUT_MS)
  }

  return {
    schedule: () => {
      if (scheduled) {
        return
      }
      scheduled = true
      // Suspended (hidden pane): remember the request but don't burn a frame;
      // setSuspended(false) replays one draw so the pane shows its latest state.
      if (suspended) {
        return
      }
      arm()
    },
    consume: () => {
      scheduled = false
      // The winner of the rAF/timer race clears the loser so it no-ops.
      clearRaf()
      clearTimer()
    },
    isScheduled: () => scheduled,
    isSuspended: () => suspended,
    setSuspended: (next: boolean) => {
      if (next === suspended) {
        return
      }
      suspended = next
      if (suspended) {
        // Drop any in-flight frame while paused; schedule() re-arms on resume.
        clearRaf()
        clearTimer()
        return
      }
      // Resuming: if a draw was wanted while paused, arm one now.
      if (scheduled) {
        arm()
      }
    },
    dispose: () => {
      scheduled = false
      clearRaf()
      clearTimer()
    }
  }
}
