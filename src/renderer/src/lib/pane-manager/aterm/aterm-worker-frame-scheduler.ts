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

export function createWorkerFrameScheduler(deps: {
  getTerm: () => SchedulerTerminal | null
  post: (state: AtermWorkerState) => void
  /** OffscreenCanvas rAF in the worker; undefined → run synchronously. */
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
