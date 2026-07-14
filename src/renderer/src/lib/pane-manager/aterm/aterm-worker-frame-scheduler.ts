import type { AtermWorkerState } from './aterm-render-worker-protocol'

const MAX_EFFECTS_TIMER_ELAPSED_MS = 250

function boundedTimerElapsedMs(now: number, previous: number): number {
  return Math.min(Math.max(0, now - previous), MAX_EFFECTS_TIMER_ELAPSED_MS)
}

// Coalesces the render worker's draws onto one rAF frame and decides when to post a STATE
// snapshot. Extracted from the worker entry to keep it under the line cap.
//
// Two behaviors it owns:
//  - Render-only frames (cursor blink/hollow): repaint the cursor without posting STATE
//    (no snapshot field tracks blink phase, so the post would be byte-identical).
//  - Hidden/suspended panes: skip the paint AND the per-output-frame post — nothing reads
//    a hidden pane's mirror before it resumes (a11y/scroll-restore/sync reads target the
//    visible pane; save uses the independent serialize cache), and resume re-posts. It
//    STILL posts when the grid dimensions change, so a resize-while-hidden keeps gridSize()
//    correct for layout persistence. The engine keeps processing bytes regardless.

type SchedulerTerminal = {
  render: () => void
  /** Advance the effects clock pre-render; true while still animating (keep rAF
   *  cadence), false once settled (the engine's idle-to-zero contract). */
  tickEffects: () => boolean
  /** Ms until the next scheduled engine wake, or undefined when active effects need rAF. */
  effectsIdleDeadlineMs: () => number | undefined
  /** Cross an armed idle deadline on the injected clock (timer-fired frames). */
  advanceEffectsBy: (dtMs: number) => void
  buildState: () => AtermWorkerState
  dimensions: () => { cols: number; rows: number; cellWidth: number; cellHeight: number }
}

/** ONE rAF loop for the whole shared worker: every pane's scheduler enqueues its draw
 *  callback here and a single rAF services all dirty panes in that frame (N panes must
 *  not book N competing rAF callbacks). Callbacks are deduped per flush — a pane's own
 *  scheduler already coalesces, so at most one entry per pane per frame. Without a
 *  native rAF (test envs) it runs synchronously, matching the per-pane fallback. */
export function createSharedWorkerRafLoop(
  raf: ((cb: () => void) => void) | undefined
): (cb: () => void) => void {
  const pending: (() => void)[] = []
  let scheduled = false
  const flush = (): void => {
    scheduled = false
    // Swap out the queue first: a callback that re-schedules lands in the NEXT frame.
    const run = pending.splice(0, pending.length)
    for (const cb of run) {
      cb()
    }
  }
  return (cb) => {
    pending.push(cb)
    if (scheduled) {
      return
    }
    scheduled = true
    if (raf) {
      raf(flush)
    } else {
      flush()
    }
  }
}

/** One pane's draw/STATE-post scheduler (the shared worker keeps one per pane). */
export type WorkerFrameScheduler = ReturnType<typeof createWorkerFrameScheduler>

export function createWorkerFrameScheduler(deps: {
  getTerm: () => SchedulerTerminal | null
  post: (state: AtermWorkerState) => void
  /** rAF-shaped scheduler — in the shared worker this is the ONE shared rAF loop
   *  (createSharedWorkerRafLoop); undefined → run synchronously. */
  raf: ((cb: () => void) => void) | undefined
}): {
  /** Schedule a coalesced draw. `postState` (default true) marks that this frame must post
   *  a STATE; blink/hollow pass false for a render-only frame. */
  schedule: (postState?: boolean) => void
  /** Post a coalesced STATE snapshot WITHOUT rendering the engine — for hover, whose
   *  underline + cursor are main-thread overlays, so the framebuffer stays byte-identical
   *  and a full render() would be pure waste. Never route a draw that changes engine
   *  render state through here. */
  scheduleStatePost: () => void
  /** Force a STATE post now — the first frame, which the loader awaits for cell metrics. */
  postNow: () => void
  /** Set the hidden/suspended flag; resuming (false) schedules a post of the latest state. */
  setSuspended: (suspended: boolean) => void
  /** Cancel the armed effects deadline timer (pane dispose). */
  dispose: () => void
  /** Interactive echo fast path: paint SYNCHRONOUSLY now (coalesced to one eager
   *  paint per frame) instead of waiting for this pane's shared-rAF tick. */
  presentNow: () => void
} {
  let suspended = false
  let drawScheduled = false
  let needStatePost = false
  // A render-free hover STATE post is queued for this frame (see scheduleStatePost).
  let hoverPostScheduled = false
  // True once an eager present painted THIS frame, so multiple interactive nudges in
  // one frame coalesce to a single synchronous paint (reset at the next frame).
  let eagerPresentedThisFrame = false
  let lastCols = -1
  let lastRows = -1
  let lastCellW = -1
  let lastCellH = -1
  let requestedDraw = 0
  let renderedDraw = 0
  // One timer covers rain cadence and sparse idle one-shots. Real state changes
  // preempt it, while rAF-only effects receive no finite engine deadline.
  let effectsTimer: ReturnType<typeof setTimeout> | null = null

  const clearEffectsTimer = (): void => {
    if (effectsTimer !== null) {
      clearTimeout(effectsTimer)
      effectsTimer = null
    }
  }

  const postState = (term: SchedulerTerminal): void => {
    const state = term.buildState()
    lastCols = state.cols
    lastRows = state.rows
    lastCellW = state.cellWidth
    lastCellH = state.cellHeight
    deps.post(state)
  }

  const drawNow = (): void => {
    const targetDraw = requestedDraw
    const term = deps.getTerm()
    if (!term) {
      return
    }
    if (suspended) {
      if (needStatePost) {
        needStatePost = false
        const dims = term.dimensions()
        if (
          dims.cols !== lastCols ||
          dims.rows !== lastRows ||
          dims.cellWidth !== lastCellW ||
          dims.cellHeight !== lastCellH
        ) {
          postState(term)
        }
      }
      return
    }
    clearEffectsTimer()
    // Advance the effects clock before the paint so this frame shows the advanced
    // state; while an effect is still animating, book a render-only follow-up frame
    // (rAF cadence). Once settled the engine reports inactive → zero rAF work, with
    // at most ONE timer armed for its next idle one-shot.
    const effectsAnimating = term.tickEffects()
    // This is the wall-clock anchor at which tickEffects advanced the injected
    // engine clock. Include render + timer lateness in the next timer charge.
    const effectsAdvancedAtMs = performance.now()
    term.render()
    if (needStatePost) {
      needStatePost = false
      postState(term)
    }
    // Consume only the requests visible at draw entry. A synchronous render can
    // enqueue newer work; that later generation must survive for the queued rAF.
    renderedDraw = Math.max(renderedDraw, targetDraw)
    const deadline = term.effectsIdleDeadlineMs()
    if (deadline !== undefined && Number.isFinite(deadline)) {
      effectsTimer = setTimeout(
        () => {
          effectsTimer = null
          const liveTerm = deps.getTerm()
          if (!liveTerm || suspended) {
            return
          }
          // Timer throttling can fire well after the requested deadline. Charge
          // the real bounded wall interval since tickEffects, then advanceBy
          // rebases its clock so the queued frame charges only its rAF delay.
          liveTerm.advanceEffectsBy(boundedTimerElapsedMs(performance.now(), effectsAdvancedAtMs))
          schedule(false)
        },
        Math.max(0, deadline)
      )
      return
    }
    if (effectsAnimating) {
      schedule(false)
    }
  }

  const schedule = (postState_ = true): void => {
    // Mark the post need BEFORE the already-scheduled guard, so a real change coalesced
    // onto a pending render-only (blink) frame still posts a STATE.
    if (postState_) {
      needStatePost = true
      // PTY/input state is authoritative and must not queue behind a rain timer.
      clearEffectsTimer()
    }
    requestedDraw++
    if (drawScheduled || !deps.getTerm()) {
      return
    }
    drawScheduled = true
    const run = (): void => {
      drawScheduled = false
      if (renderedDraw < requestedDraw) {
        drawNow()
      }
    }
    if (deps.raf) {
      deps.raf(run)
    } else {
      run()
    }
  }

  // Hover STATE-only path: setHover changed the link/cursor OUTCOME but no engine render
  // state, so post the fresh snapshot WITHOUT term.render()/tickEffects/arming an effects
  // timer (the framebuffer is byte-identical; the underline + cursor are main-thread
  // overlays). This is the ONLY caller that skips render — content/effect draws must use
  // schedule(). A real draw already queued this frame supersedes the post-only frame (its
  // STATE carries the same fresh hover), so defer to it instead of double-posting.
  const scheduleStatePost = (): void => {
    if (suspended) {
      // Hidden pane posts nothing (nothing reads its visible mirror; resume re-posts), and a
      // hover carries no dimension change — mirror drawNow's suspended path.
      return
    }
    if (drawScheduled) {
      // A draw is already queued this frame: make it post the fresh hover after it renders,
      // rather than booking a redundant post-only frame.
      needStatePost = true
      return
    }
    if (hoverPostScheduled) {
      return
    }
    hoverPostScheduled = true
    const run = (): void => {
      hoverPostScheduled = false
      const term = deps.getTerm()
      if (!term || suspended) {
        return
      }
      if (drawScheduled) {
        // A real draw got queued after us — let it carry the post (with its render).
        needStatePost = true
        return
      }
      postState(term)
    }
    if (deps.raf) {
      deps.raf(run)
    } else {
      run()
    }
  }

  // Interactive echo fast path (the main-thread presentNow nudge): render NOW in the
  // worker so the glyph catches the current compositor frame, instead of waiting a
  // full shared-rAF tick (~16.7ms@60Hz) behind the sibling 'process' schedule that
  // already coalesced the nudge away. Bulk output keeps flowing through schedule() on
  // 'process', so this adds at most one synchronous paint per frame.
  const presentNow = (): void => {
    const term = deps.getTerm()
    if (!term || suspended) {
      // Hidden pane / engine still building: fall back to the coalesced path
      // (resume / next frame repaints the latest state).
      schedule()
      return
    }
    if (eagerPresentedThisFrame) {
      // Already eagerly painted this frame — coalesce any newer state onto the rAF.
      schedule()
      return
    }
    // Preserve the STATE post the old 'draw'→schedule(true) carried so the main-thread
    // mirror still updates on this frame.
    needStatePost = true
    requestedDraw++
    eagerPresentedThisFrame = true
    drawNow()
    // Re-open the eager gate at the next frame boundary (a harmless one-shot even when
    // no other draw is armed; without a native rAF each echo just presents synchronously).
    if (deps.raf) {
      deps.raf(() => {
        eagerPresentedThisFrame = false
      })
    } else {
      eagerPresentedThisFrame = false
    }
  }

  return {
    schedule,
    scheduleStatePost,
    postNow: () => {
      needStatePost = true
      requestedDraw++
      drawNow()
    },
    setSuspended: (next) => {
      suspended = next
      if (next) {
        // A hidden pane paints nothing; drop the armed one-shot (resume re-arms).
        clearEffectsTimer()
        return
      }
      schedule()
    },
    presentNow,
    dispose: clearEffectsTimer
  }
}
