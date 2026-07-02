import type { AtermWorkerState } from './aterm-render-worker-protocol'

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
  /** Force a STATE post now — the first frame, which the loader awaits for cell metrics. */
  postNow: () => void
  /** Set the hidden/suspended flag; resuming (false) schedules a post of the latest state. */
  setSuspended: (suspended: boolean) => void
} {
  let suspended = false
  let drawScheduled = false
  let needStatePost = false
  let lastCols = -1
  let lastRows = -1
  let lastCellW = -1
  let lastCellH = -1

  const postState = (term: SchedulerTerminal): void => {
    const state = term.buildState()
    lastCols = state.cols
    lastRows = state.rows
    lastCellW = state.cellWidth
    lastCellH = state.cellHeight
    deps.post(state)
  }

  const drawNow = (): void => {
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
    term.render()
    if (needStatePost) {
      needStatePost = false
      postState(term)
    }
  }

  const schedule = (postState_ = true): void => {
    // Mark the post need BEFORE the already-scheduled guard, so a real change coalesced
    // onto a pending render-only (blink) frame still posts a STATE.
    if (postState_) {
      needStatePost = true
    }
    if (drawScheduled || !deps.getTerm()) {
      return
    }
    drawScheduled = true
    const run = (): void => {
      drawScheduled = false
      drawNow()
    }
    if (deps.raf) {
      deps.raf(run)
    } else {
      run()
    }
  }

  return {
    schedule,
    postNow: () => {
      needStatePost = true
      drawNow()
    },
    setSuspended: (next) => {
      suspended = next
      if (!next) {
        schedule()
      }
    }
  }
}
