import { SYNC_FIT_PANES_EVENT } from '@/constants/terminal'
import { makePaneKey } from '../../../../../shared/stable-pane-id'
import {
  getLivePaneManagersForTab,
  getRegisteredTabPaneManagerTabIds
} from '../pane-manager-registry'
import {
  chromeFrameOrigin,
  chromeOutsideRects,
  chromeStripRects,
  type AtermDeviceRect
} from './aterm-chrome-box'
import { attachAtermDprTracker } from './aterm-dpr-tracker'
import { atermSpillOverlay, type AtermSpillOverlay } from './aterm-spill-overlay'
import type { SpillPaneGeometry } from './aterm-spill-pane-scratch'

// Event-driven geometry tracker for the spill overlay: ONE rAF-coalesced
// measure pass (the syncPaneTitleOverlayRects pattern from TerminalPane.tsx)
// re-derives every registered pane's frameOrigin/clipRect/outsideRects in
// integer device px via aterm-chrome-box. It never runs per frame — only on
// resize/layout/dpr triggers — and it exists only while the overlay layer is
// mounted (i.e. at least one pane is registered), so idle cost is zero.
//
// Trigger-name findings vs the plan:
// - "paneLayoutRevision bumps" is TerminalPane-local React state fed by
//   PaneManager onLayoutChanged callbacks — not subscribable from here. The
//   DOM-projected equivalent is a childList MutationObserver on the surfaces
//   container: reorders/reparents without a resize are exactly DOM moves.
// - "isVisible/isWorktreeActive flags" are TerminalPane props; their DOM
//   projection is aria-hidden/inert on the worktree surface wrapper
//   (WorktreeSplitSurface), so visibility reads that — never rect size,
//   because hidden-but-measurable worktrees keep real rects at opacity 0.

export type AtermSpillGeometryTrackerDeps = {
  /** The terminal-surfaces container the overlay canvas fills; its box anchors
   *  all overlay-space coordinates. */
  container: HTMLElement
  overlay?: AtermSpillOverlay
  getDpr?: () => number
}

export type AtermSpillGeometryTracker = { dispose: () => void }

/** Pushed for registered panes that cannot be measured (no live pane, no grid
 *  canvas, or an unlaid-out 0-size box): paints nothing, keeps registration. */
const HIDDEN_SPILL_GEOMETRY: SpillPaneGeometry = Object.freeze({
  frameOrigin: { x: 0, y: 0 },
  clipRect: { x: 0, y: 0, width: 0, height: 0 },
  stripRects: [],
  outsideRects: [],
  visible: false
})

function roundDeviceRect(rect: DOMRect, origin: DOMRect, dpr: number): AtermDeviceRect {
  // ONE rounding at the measured CSS box; all derived chrome math below stays
  // integer, so strips and clip can never drift apart by a device pixel.
  return {
    x: Math.round((rect.left - origin.left) * dpr),
    y: Math.round((rect.top - origin.top) * dpr),
    width: Math.round(rect.width * dpr),
    height: Math.round(rect.height * dpr)
  }
}

export function startAtermSpillGeometryTracker(
  deps: AtermSpillGeometryTrackerDeps
): AtermSpillGeometryTracker {
  const { container } = deps
  const overlay = deps.overlay ?? atermSpillOverlay
  const getDpr = deps.getDpr ?? ((): number => window.devicePixelRatio || 1)
  let disposed = false
  let pendingFrame: number | null = null
  let trackedDpr = getDpr()
  const observedPaneEls = new Set<Element>()

  const measurePane = (
    paneKey: string,
    paneEl: HTMLElement,
    containerRect: DOMRect,
    dpr: number
  ): SpillPaneGeometry => {
    const chrome = overlay.getPaneChrome(paneKey)
    const canvasEl = paneEl.querySelector('canvas[data-testid="aterm-canvas"]')
    if (!chrome || !canvasEl) {
      return HIDDEN_SPILL_GEOMETRY
    }
    const frameRect = canvasEl.getBoundingClientRect()
    if (frameRect.width <= 0 || frameRect.height <= 0) {
      return HIDDEN_SPILL_GEOMETRY
    }
    const pad = chrome.chromePadPx
    const head = chrome.chromeHeadPx
    // The drawers pin the canvas box to the chrome-padded FRAME (negative
    // margins pull it up-left of the grid), so the canvas rect measures the
    // frame and the grid box is recovered by the inverse chrome offsets.
    const frame = roundDeviceRect(frameRect, containerRect, dpr)
    const gridBox = {
      x: frame.x + pad,
      y: frame.y + pad + head,
      width: Math.max(0, frame.width - 2 * pad),
      height: Math.max(0, frame.height - 2 * pad - head)
    }
    const clipRect = roundDeviceRect(paneEl.getBoundingClientRect(), containerRect, dpr)
    return {
      frameOrigin: chromeFrameOrigin(gridBox, pad, head),
      clipRect,
      stripRects: chromeStripRects(gridBox, pad, head),
      outsideRects: chromeOutsideRects(gridBox, pad, head, clipRect),
      visible: paneEl.closest('[aria-hidden="true"], [inert]') === null
    }
  }

  const syncPaneObservers = (liveEls: ReadonlySet<Element>): void => {
    if (!resizeObserver) {
      return
    }
    for (const el of observedPaneEls) {
      if (!liveEls.has(el)) {
        resizeObserver.unobserve(el)
        observedPaneEls.delete(el)
      }
    }
    for (const el of liveEls) {
      // Only NEW elements are observed: ResizeObserver fires once per observe()
      // call, so re-observing everything each pass would loop forever.
      if (!observedPaneEls.has(el)) {
        observedPaneEls.add(el)
        resizeObserver.observe(el)
      }
    }
  }

  const measureNow = (): void => {
    if (disposed) {
      return
    }
    const dpr = getDpr()
    const containerRect = container.getBoundingClientRect()
    overlay.setOverlayBox({
      widthPx: Math.round(containerRect.width * dpr),
      heightPx: Math.round(containerRect.height * dpr)
    })
    const unseen = new Set(overlay.getPaneKeys())
    const liveEls = new Set<Element>()
    if (unseen.size > 0) {
      for (const tabId of getRegisteredTabPaneManagerTabIds()) {
        for (const manager of getLivePaneManagersForTab(tabId)) {
          let panes: ReturnType<typeof manager.getPanes>
          try {
            panes = manager.getPanes()
          } catch {
            // A replacement lifecycle can overlap a manager already tearing
            // down (registry convention); measure the remaining live ones.
            continue
          }
          for (const pane of panes) {
            let paneKey: string
            try {
              paneKey = makePaneKey(tabId, pane.leafId)
            } catch {
              continue
            }
            if (!unseen.delete(paneKey)) {
              continue
            }
            const paneEl = pane.container
            if (!paneEl) {
              overlay.updateGeometry(paneKey, HIDDEN_SPILL_GEOMETRY)
              continue
            }
            liveEls.add(paneEl)
            overlay.updateGeometry(paneKey, measurePane(paneKey, paneEl, containerRect, dpr))
          }
        }
      }
      // Registered panes with no live manager pane (cold-parked, mid-teardown):
      // stop painting them but keep the registration for their return.
      for (const paneKey of unseen) {
        overlay.updateGeometry(paneKey, HIDDEN_SPILL_GEOMETRY)
      }
    }
    syncPaneObservers(liveEls)
  }

  const scheduleMeasure = (): void => {
    if (disposed || pendingFrame !== null) {
      return
    }
    pendingFrame = requestAnimationFrame(() => {
      pendingFrame = null
      measureNow()
    })
  }

  const resizeObserver =
    typeof ResizeObserver === 'function' ? new ResizeObserver(scheduleMeasure) : null
  resizeObserver?.observe(container)
  const mutationObserver =
    typeof MutationObserver === 'function' ? new MutationObserver(scheduleMeasure) : null
  mutationObserver?.observe(container, { childList: true, subtree: true })
  window.addEventListener('resize', scheduleMeasure)
  window.addEventListener(SYNC_FIT_PANES_EVENT, scheduleMeasure)
  const dprTracker = attachAtermDprTracker({
    getDpr: () => trackedDpr,
    onDprChange: (next) => {
      trackedDpr = next
      scheduleMeasure()
    },
    isDisposed: () => disposed
  })
  // Chrome changes and new registrations re-shape the strips without any DOM
  // event, so the registry itself is a measure trigger.
  const unsubscribeRegistry = overlay.subscribe(scheduleMeasure)
  scheduleMeasure()

  return {
    dispose: () => {
      disposed = true
      if (pendingFrame !== null) {
        cancelAnimationFrame(pendingFrame)
        pendingFrame = null
      }
      resizeObserver?.disconnect()
      mutationObserver?.disconnect()
      window.removeEventListener('resize', scheduleMeasure)
      window.removeEventListener(SYNC_FIT_PANES_EVENT, scheduleMeasure)
      dprTracker.dispose()
      unsubscribeRegistry()
      observedPaneEls.clear()
    }
  }
}
