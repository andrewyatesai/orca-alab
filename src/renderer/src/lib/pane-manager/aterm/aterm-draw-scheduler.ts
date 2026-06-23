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
  /** Cancel any pending rAF/timer (call on dispose). */
  dispose: () => void
}

// 33ms ≈ one 30fps frame: long enough that the rAF usually wins on a visible
// window, short enough that a hidden/occluded pane still paints promptly.
const BACKSTOP_TIMEOUT_MS = 33

export function createAtermDrawScheduler(runDraw: () => void): AtermDrawScheduler {
  let scheduled = false
  let timeoutId: ReturnType<typeof setTimeout> | null = null

  const clearTimer = (): void => {
    if (timeoutId !== null) {
      clearTimeout(timeoutId)
      timeoutId = null
    }
  }

  return {
    schedule: () => {
      if (scheduled) {
        return
      }
      scheduled = true
      requestAnimationFrame(runDraw)
      // Replace any prior backstop so timers never accumulate across schedules.
      clearTimer()
      timeoutId = setTimeout(runDraw, BACKSTOP_TIMEOUT_MS)
    },
    consume: () => {
      scheduled = false
      // The winner of the rAF/timer race clears the loser so it no-ops.
      clearTimer()
    },
    isScheduled: () => scheduled,
    dispose: () => {
      scheduled = false
      clearTimer()
    }
  }
}
