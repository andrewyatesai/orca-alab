import { createDividerFlexFrameScheduler } from '../../lib/pane-manager/pane-divider-drag'

export type SplitResizeDragCleanup = (options?: { commit?: boolean; silent?: boolean }) => void

export type SplitResizeDragOptions = {
  handle: HTMLElement
  pointerId: number
  isHorizontal: boolean
  minRatio: number
  maxRatio: number
  onRatioChange: (ratio: number) => void
  // Fired once when the gesture ends (pointerup/cancel/lostpointercapture) so
  // the owning component can drop its `is-dragging` state. Skipped on `silent`
  // teardown (e.g. unmount) to avoid a React state update on a dead component.
  onEnd?: () => void
  requestFrame?: (callback: FrameRequestCallback) => number
  cancelFrame?: (handle: number) => void
}

// Coordinates a split-divider drag entirely on the DOM: the live ratio is
// written to the two flanking panes' `flex` per animation frame, and the store
// is committed exactly once on release. Returns a cleanup, or null when the
// divider has no resizable siblings to size. Kept out of the React component so
// the pointer orchestration is unit-testable without a layout-aware DOM.
export function beginSplitResizeDrag({
  handle,
  pointerId,
  isHorizontal,
  minRatio,
  maxRatio,
  onRatioChange,
  onEnd,
  requestFrame,
  cancelFrame
}: SplitResizeDragOptions): SplitResizeDragCleanup | null {
  const container = handle.parentElement
  const prevEl = handle.previousElementSibling as HTMLElement | null
  const nextEl = handle.nextElementSibling as HTMLElement | null
  if (!container || !prevEl || !nextEl) {
    return null
  }

  // Why: the store ratio never changes mid-drag, so a cancelled/blurred gesture
  // must restore these flex bases itself — React won't re-render to do it.
  const prevInitialFlex = prevEl.style.flex
  const nextInitialFlex = nextEl.style.flex
  let latestRatio: number | null = null

  // Why: raw pointermove can fire ~100-120/s on high-refresh input. Coalescing
  // the live flex writes to one update per animation frame keeps the divider
  // smooth without rewriting the global store on every event.
  const flexScheduler = createDividerFlexFrameScheduler({
    apply: (prevFlex, nextFlex) => {
      prevEl.style.flex = `${prevFlex} 1 0%`
      nextEl.style.flex = `${nextFlex} 1 0%`
    },
    requestFrame,
    cancelFrame
  })

  const onPointerMove = (moveEvent: PointerEvent): void => {
    if (!handle.hasPointerCapture(pointerId)) {
      return
    }
    const rect = container.getBoundingClientRect()
    const rawRatio = isHorizontal
      ? (moveEvent.clientX - rect.left) / rect.width
      : (moveEvent.clientY - rect.top) / rect.height
    const ratio = Math.min(maxRatio, Math.max(minRatio, rawRatio))
    latestRatio = ratio
    flexScheduler.schedule(ratio, 1 - ratio)
  }

  let cleaned = false
  const cleanup: SplitResizeDragCleanup = ({ commit = false, silent = false } = {}): void => {
    if (cleaned) {
      return
    }
    cleaned = true

    if (commit) {
      // Land the final frame synchronously so the DOM matches the ratio we
      // commit to the store; the ensuing re-render then rewrites the same flex.
      flexScheduler.flush()
    } else {
      flexScheduler.cancel()
      prevEl.style.flex = prevInitialFlex
      nextEl.style.flex = nextInitialFlex
    }

    try {
      if (handle.hasPointerCapture(pointerId)) {
        handle.releasePointerCapture(pointerId)
      }
    } catch {
      // Best effort: unmount cleanup can run after Chromium has already dropped capture.
    }
    handle.removeEventListener('pointermove', onPointerMove)
    handle.removeEventListener('pointerup', onPointerUp)
    handle.removeEventListener('pointercancel', onPointerCancel)
    handle.removeEventListener('lostpointercapture', onLostPointerCapture)

    // Why: commit the resized ratio to the store exactly once, on release, and
    // only when the pointer actually moved (a bare click leaves layout as-is).
    if (commit && latestRatio !== null) {
      onRatioChange(latestRatio)
    }
    if (!silent) {
      onEnd?.()
    }
  }

  const onPointerUp = (): void => cleanup({ commit: true })
  const onPointerCancel = (): void => cleanup({ commit: false })
  const onLostPointerCapture = (): void => cleanup({ commit: false })

  handle.setPointerCapture(pointerId)
  handle.addEventListener('pointermove', onPointerMove)
  handle.addEventListener('pointerup', onPointerUp)
  handle.addEventListener('pointercancel', onPointerCancel)
  handle.addEventListener('lostpointercapture', onLostPointerCapture)

  return cleanup
}
