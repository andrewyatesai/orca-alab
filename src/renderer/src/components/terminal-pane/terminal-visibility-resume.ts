import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import type { ScrollState } from '@/lib/pane-manager/pane-manager-types'
import {
  flushTerminalOutput,
  requestTerminalBacklogRecovery
} from '@/lib/pane-manager/pane-terminal-output-scheduler'
import { restoreScrollStateAfterLayout } from '@/lib/pane-manager/pane-scroll'
import { fitAndFocusPanes, fitPanes, focusActivePane } from './pane-helpers'

const VISIBLE_RESUME_FLUSH_CHARS = 256 * 1024

export type TerminalHiddenReason = 'surface' | 'tab'

type ResumeTerminalVisibilityArgs = {
  manager: PaneManager
  isActive: boolean
  wasVisible: boolean
  shouldUseLightTabResume: boolean
  captureViewportPositions: (useRememberedSnapshots: boolean) => Map<number, ScrollState>
  withSuppressedScrollTracking: (callback: () => void) => void
}

type HideTerminalVisibilityArgs = {
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

export function resumeTerminalVisibility({
  manager,
  isActive,
  wasVisible,
  shouldUseLightTabResume,
  captureViewportPositions,
  withSuppressedScrollTracking
}: ResumeTerminalVisibilityArgs): void {
  // Why: WebGL resume can disturb xterm's viewport bookkeeping before the
  // post-resume fit runs. Capture numeric viewport positions first; the
  // restore path avoids content matching so duplicate agent log lines do
  // not jump to the wrong history entry.
  const viewportPositions = captureViewportPositions(!wasVisible)
  withSuppressedScrollTracking(() => {
    if (shouldUseLightTabResume) {
      // Why: intra-worktree tab switches only toggle the overlay. Still request
      // hidden-output recovery: agent TUIs can suppress hidden bytes until the
      // pane is foregrounded.
      requestLightTabBacklogRecovery(manager)
      if (isActive) {
        focusActivePane(manager)
      }
    } else {
      resumeTerminalVisibilityHeavy(manager, isActive)
    }
    restoreTerminalViewportPositions(manager, viewportPositions)
  })
}

export function hideTerminalVisibility({
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
    return { hiddenReason: 'surface', renderingSuspended: true }
  }
  if (!hasCompletedVisibleResume && wasVisible && wasWorktreeActive && isWorktreeActive) {
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
  // Single fit on resume. Background bytes have been pushed into the engine
  // above, so this fit only absorbs container dimension changes that
  // happened while hidden (e.g. sidebar toggle on another worktree).
  if (isActive) {
    fitAndFocusPanes(manager)
  } else {
    fitPanes(manager)
  }
}

function restoreTerminalViewportPositions(
  manager: PaneManager,
  viewportPositions: Map<number, ScrollState>
): void {
  for (const pane of manager.getPanes()) {
    const position = viewportPositions.get(pane.id)
    if (position) {
      restoreScrollStateAfterLayout(pane.terminal, position)
    }
  }
}
