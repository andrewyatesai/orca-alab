import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import type { ScrollState } from '@/lib/pane-manager/pane-manager-types'
import {
  flushTerminalOutput,
  requestTerminalBacklogRecovery
} from '@/lib/pane-manager/pane-terminal-output-scheduler'
import {
  resetAllTerminalWebglAtlases,
  resetAndRefreshAllTerminalWebglAtlases
} from '@/lib/pane-manager/pane-manager-registry'
import {
  beginSuppressScrollIntentWrites,
  endSuppressScrollIntentWrites,
  enforceTerminalCurrentScrollIntent,
  syncTerminalScrollIntentFromViewport
} from '@/lib/pane-manager/terminal-scroll-intent'
import { fitAndFocusPanes, fitPanes, focusActivePane } from './pane-helpers'
import { scheduleTabRevealWebglAtlasRecovery } from './terminal-webgl-atlas-recovery'

// Re-anchor schedule after a resume: two rAFs + an 80ms backstop, matching
// restoreScrollStateAfterLayout / syncTerminalScrollIntentSoon, so the durable pin is
// re-applied once the cold-restore replay flood has settled. The write-freeze is held
// until the backstop fires.
const RESUME_REANCHOR_BACKSTOP_MS = 80

const VISIBLE_RESUME_FLUSH_CHARS = 256 * 1024
const WINDOW_WAKE_FLUSH_CHARS = 64 * 1024

export type TerminalHiddenReason = 'surface' | 'tab'

type ResumeTerminalVisibilityArgs = {
  manager: PaneManager
  isActive: boolean
  wasVisible: boolean
  shouldUseLightTabResume: boolean
  captureViewportPositions: (useRememberedSnapshots: boolean) => Map<number, ScrollState>
}

type HideTerminalVisibilityArgs = {
  manager: PaneManager
  wasVisible: boolean
  wasWorktreeActive: boolean
  isWorktreeActive: boolean
  hasCompletedVisibleResume: boolean
  captureViewportPositions: (useRememberedSnapshots: boolean) => Map<number, ScrollState>
}

type HideTerminalVisibilityResult = {
  hiddenReason: TerminalHiddenReason | null
  renderingSuspended: boolean
}

type RecoverVisibleTerminalWindowWakeArgs = {
  manager: PaneManager
  isActive: boolean
  clearGlyphAtlases: boolean
}

export function resumeTerminalVisibility({
  manager,
  isActive,
  wasVisible,
  shouldUseLightTabResume,
  captureViewportPositions
}: ResumeTerminalVisibilityArgs): void {
  syncTerminalViewportIntents(manager)
  // Why: WebGL resume can disturb xterm's viewport bookkeeping before the
  // post-resume fit runs. Capture numeric viewport positions first; the
  // restore path avoids content matching so duplicate agent log lines do
  // not jump to the wrong history entry.
  captureViewportPositions(!wasVisible)
  // FREEZE intent writes across the WHOLE resume window — the synchronous flush/fit/
  // enforce AND the async cold-restore replay flood that follows. The replay clears
  // and regrows the buffer; without the freeze a transient empty/regrowing buffer (and
  // the syncTerminalScrollIntentSoon timers landing mid-replay) overwrite the durable
  // ABSOLUTE pin with a position relative to the rebuilt bottom, so the restore lands
  // on the wrong content (the worktree-switch scroll-jump). enforce* still SCROLLS
  // while frozen, so the pin is re-anchored, not lost. Released on a bounded backstop
  // (and via finally) so a throw can never strand the freeze on.
  beginSuppressScrollIntentWrites()
  let released = false
  const release = (): void => {
    if (!released) {
      released = true
      endSuppressScrollIntentWrites()
    }
  }
  try {
    if (shouldUseLightTabResume) {
      // Why: intra-worktree tab switches only toggle the overlay. Still request
      // hidden-output recovery: agent TUIs can suppress hidden bytes until the
      // pane is foregrounded.
      requestLightTabBacklogRecovery(manager)
      // Why: reveal recovery must be immediate, not the terminal-output debounce
      // — a background agent streaming in another pane must not defer this tab's
      // atlas rebuild.
      scheduleTabRevealWebglAtlasRecovery()
      if (isActive) {
        focusActivePane(manager)
      }
    } else {
      resumeTerminalVisibilityHeavy(manager, isActive)
    }
    enforceTerminalViewportIntents(manager)
    if (!shouldUseLightTabResume) {
      // Why: after a heavy resume from hidden, force every live manager to
      // re-present its aterm grid so returning panes repaint a fresh frame.
      resetAllTerminalWebglAtlases()
    }
  } finally {
    // Re-anchor the durable absolute pin AFTER the replay flood settles (two rAFs +
    // an 80ms backstop), THEN release the write-freeze. Scheduled from `finally` so a
    // throw in the resume body still releases the freeze on the backstop. rAF is
    // guarded (absent in headless/test environments) and falls back to a timer; the
    // 80ms setTimeout backstop ALWAYS fires, so the freeze is guaranteed released.
    const raf = (cb: () => void): void => {
      if (typeof requestAnimationFrame === 'function') {
        requestAnimationFrame(cb)
      } else {
        setTimeout(cb, 0)
      }
    }
    const reanchor = (): void => enforceTerminalViewportIntents(manager)
    raf(() => {
      reanchor()
      raf(reanchor)
    })
    setTimeout(() => {
      reanchor()
      release()
    }, RESUME_REANCHOR_BACKSTOP_MS)
  }
}

export function hideTerminalVisibility({
  manager,
  wasVisible,
  wasWorktreeActive,
  isWorktreeActive,
  hasCompletedVisibleResume,
  captureViewportPositions
}: HideTerminalVisibilityArgs): HideTerminalVisibilityResult {
  const surfaceBecameHidden = wasWorktreeActive && !isWorktreeActive
  if (wasVisible) {
    // Why: hidden DOM/layout churn can mutate the viewport before the pane
    // becomes visible again. Preserve the last visible position.
    captureViewportPositions(false)
  }
  if (!isWorktreeActive && (wasVisible || surfaceBecameHidden)) {
    // Pause draw scheduling while hidden: engines keep ingesting PTY bytes
    // but paint no frames (and hold no GPU work) until resumed.
    manager.suspendRendering()
    return { hiddenReason: 'surface', renderingSuspended: true }
  }
  if (!hasCompletedVisibleResume && wasVisible && wasWorktreeActive && isWorktreeActive) {
    // Why: the visibility hook starts wasVisible=true so terminal tabs that
    // first mount hidden still stop painting instead of drawing offscreen.
    manager.suspendRendering()
    return { hiddenReason: 'tab', renderingSuspended: true }
  }
  if (wasVisible && isWorktreeActive) {
    return { hiddenReason: 'tab', renderingSuspended: false }
  }
  if (!isWorktreeActive) {
    return { hiddenReason: 'surface', renderingSuspended: false }
  }
  return { hiddenReason: null, renderingSuspended: false }
}

export function recoverVisibleTerminalWindowWake({
  manager,
  isActive,
  clearGlyphAtlases
}: RecoverVisibleTerminalWindowWakeArgs): void {
  // Why: macOS screensaver/display wake can leave xterm visible but with a
  // stale renderer/input surface; Orca's own hidden-state resume never runs.
  for (const pane of manager.getPanes()) {
    requestTerminalBacklogRecovery(pane.terminal)
    flushTerminalOutput(pane.terminal, { maxChars: WINDOW_WAKE_FLUSH_CHARS })
  }
  syncTerminalViewportIntents(manager)
  manager.resumeRendering()
  if (isActive) {
    fitAndFocusPanes(manager)
  } else {
    fitPanes(manager)
  }
  enforceTerminalViewportIntents(manager)
  if (clearGlyphAtlases) {
    // Why: only a genuine display wake takes the heavy path — reset AND refresh
    // every pane's aterm grid, since a real wake can corrupt the GPU surface.
    // Plain refocus (alt-tab) is frequent and must not pay this cross-manager cost.
    resetAndRefreshAllTerminalWebglAtlases()
  } else {
    // Why: a plain refocus just re-presents the current aterm frame — the
    // atlas-preserving equivalent that avoids re-arming the heavy refresh churn.
    resetAllTerminalWebglAtlases()
  }
}

function requestLightTabBacklogRecovery(manager: PaneManager): void {
  for (const pane of manager.getPanes()) {
    requestTerminalBacklogRecovery(pane.terminal)
  }
}

function resumeTerminalVisibilityHeavy(manager: PaneManager, isActive: boolean): void {
  // Why: hidden panes can accumulate large PTY bursts while Chromium is
  // occluded. Drain a bounded slice before fitting; the scheduler keeps
  // ordering and continues the rest asynchronously so return-to-app does
  // not beachball behind an entire backlog.
  for (const pane of manager.getPanes()) {
    requestTerminalBacklogRecovery(pane.terminal)
    flushTerminalOutput(pane.terminal, { maxChars: VISIBLE_RESUME_FLUSH_CHARS })
  }
  syncTerminalViewportIntents(manager)
  // Resume draw scheduling immediately so the terminal shows its last-known
  // state on the first painted frame (panes may have been suspended while
  // hidden, or created suspended via initialRenderingSuspended).
  manager.resumeRendering()
  // Single fit on resume. Background bytes have been pushed into the engine
  // above, so this fit only absorbs container dimension changes that
  // happened while hidden (e.g. sidebar toggle on another worktree).
  if (isActive) {
    fitAndFocusPanes(manager)
  } else {
    fitPanes(manager)
  }
}

function enforceTerminalViewportIntents(manager: PaneManager): void {
  for (const pane of manager.getPanes()) {
    enforceTerminalCurrentScrollIntent(pane.terminal)
  }
}

function syncTerminalViewportIntents(manager: PaneManager): void {
  for (const pane of manager.getPanes()) {
    // Why: native scrollback trimming moves a pinned viewport content-stably.
    // Capture that live position before resume/fit can disturb it.
    syncTerminalScrollIntentFromViewport(pane.terminal)
  }
}
