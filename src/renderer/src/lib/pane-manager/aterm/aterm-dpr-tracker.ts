/** Tracks devicePixelRatio changes for an aterm pane (M2). The window can move to
 *  a different-DPI monitor, which changes devicePixelRatio; the CSS<->device
 *  mapping and grid sizing are derived from dpr, so they must follow it.
 *
 *  A matchMedia (resolution: Ndppx) 'change' listener (re-armed after each change,
 *  since a single query only fires for ITS own dpr) is the standard detection.
 *  On change we read the new dpr, hand it back to the controller (which propagates
 *  it to the pointer/scroll/mouse hit-testers and re-resizes + redraws).
 *
 *  NOTE: the engine DOES re-rasterize on a font-px change — `term.set_px` re-derives
 *  cell metrics and drops the glyph atlas — so the only requirement for crisp glyphs
 *  is that set_px is re-invoked at the LIVE dpr. matchMedia's exact-integer-dppx
 *  'change' can miss the window's initial backing-scale settle (esp. headless), so
 *  this tracker is only one of three reconcile triggers; the grid reflow's
 *  ResizeObserver and its per-frame reconcileIfNeeded (run from the draw loop) are
 *  the backstops that guarantee the engine reaches the settled dpr. */
export type AtermDprTrackerDeps = {
  /** Current dpr the pane is using (so we only react to an actual change). */
  getDpr: () => number
  /** Apply a new dpr: update hit-testers, reflow the grid, and redraw. */
  onDprChange: (nextDpr: number) => void
  isDisposed: () => boolean
}

export type AtermDprTracker = {
  dispose: () => void
}

/** Attach the resolution listener. Guards against a missing matchMedia (some
 *  headless / SSH-forwarded renderers) so it never breaks pane creation. */
export function attachAtermDprTracker(deps: AtermDprTrackerDeps): AtermDprTracker {
  const { getDpr, onDprChange, isDisposed } = deps
  let mediaQuery: MediaQueryList | null = null

  const handleChange = (): void => {
    if (isDisposed()) {
      return
    }
    const nextDpr = window.devicePixelRatio || 1
    if (nextDpr !== getDpr()) {
      onDprChange(nextDpr)
    }
    // Re-arm: the previous query only fires for its own dpr, so a fresh query at
    // the new dpr is needed to catch the next monitor move.
    arm()
  }

  const arm = (): void => {
    if (isDisposed() || typeof window.matchMedia !== 'function') {
      return
    }
    mediaQuery?.removeEventListener('change', handleChange)
    mediaQuery = window.matchMedia(`(resolution: ${getDpr()}dppx)`)
    mediaQuery.addEventListener('change', handleChange)
  }

  arm()

  return {
    dispose: () => {
      mediaQuery?.removeEventListener('change', handleChange)
    }
  }
}
