import { paintAtermLinkUnderline, type AtermHoveredLinkSpan } from './aterm-link-underline-overlay'
import { SEARCH_ACTIVE_FILL, SEARCH_MATCH_FILL } from './aterm-search-overlay'
import type { AtermWorkerState } from './aterm-render-worker-protocol'

/** The single-engine worker owns the canvas (CPU 2d blit OR GPU WebGL2 present), so
 *  the search-highlight + link-underline OVERLAYS — which need a 2d fillRect/stroke —
 *  paint onto a SEPARATE stacked 2d canvas the main thread positions exactly over the
 *  pane canvas. Driven entirely by the worker's per-frame STATE snapshot (match rects +
 *  hovered link, both computed in the worker), so it works identically for the CPU and
 *  GPU worker paths. */
export type AtermWorkerOverlay = {
  /** Repaint highlights + underline from the latest worker state. */
  paint: (state: AtermWorkerState) => void
  dispose: () => void
}

export function createAtermWorkerOverlay(
  paneCanvas: HTMLCanvasElement,
  /** Theme fg (0x00RRGGBB) — the hover-underline colour; read live so a re-theme
   *  recolours it without rebuilding the pane. */
  getFgColor: () => number,
  /** The APPLIED (reconciled) dpr the worker rendered this framebuffer at — NOT live
   *  window.devicePixelRatio, which can diverge during a DPI settle / fractional dpr and
   *  drift the highlights/underline off their cells. Mirrors aterm-search-overlay-canvas. */
  getDpr: () => number,
  /** The main-thread hovered link span (provider links live outside the worker's
   *  link detection, so their underline can only paint here), or null. */
  getMainThreadSpan: () => AtermHoveredLinkSpan | null = () => null
): AtermWorkerOverlay {
  const overlay = document.createElement('canvas')
  overlay.dataset.testid = 'aterm-worker-overlay' // e2e locator
  // Whether the last paint drew anything: lets an idle pane (no search, no hovered link)
  // skip the full-canvas clearRect entirely — only the content→empty transition needs the
  // one final clear. The CSS box size last written, to skip redundant style writes.
  let hadContent = false
  let lastCssWidth = -1
  let lastCssHeight = -1
  // Stack exactly over the pane canvas; transparent + click-through so it never
  // intercepts selection/scroll/link events (those stay on the pane canvas).
  overlay.style.position = 'absolute'
  overlay.style.left = '0'
  overlay.style.top = '0'
  overlay.style.pointerEvents = 'none'
  overlay.style.display = 'block'
  paneCanvas.parentElement?.appendChild(overlay)
  const ctx = overlay.getContext('2d')

  return {
    paint: (state) => {
      if (!ctx) {
        return
      }
      // Match the GRID's device-pixel size (CSS = device/dpr) so the rects — already
      // GRID-relative device px from the worker — land on the right cells. The frame
      // dims include the effects chrome, but this overlay anchors at the parent
      // origin (= the grid origin; the pane canvas takes the negative margins), so
      // it must stay grid-only or every rect would skew by the chrome offset.
      const width = state.width - 2 * state.chromePadPx
      const height = state.height - 2 * state.chromePadPx - state.chromeHeadPx
      if (width <= 0 || height <= 0) {
        return
      }
      if (overlay.width !== width || overlay.height !== height) {
        // Resizing the backing store clears it, so the prior content is already gone.
        overlay.width = width
        overlay.height = height
        hadContent = false
      }
      // The dpr the worker rendered at (see getDpr), NOT live devicePixelRatio.
      const dpr = getDpr() || 1
      // Only touch the CSSOM box when it actually changed (avoids two `${n}px` string
      // allocations + style writes per frame in the steady state).
      const cssWidth = width / dpr
      const cssHeight = height / dpr
      if (cssWidth !== lastCssWidth || cssHeight !== lastCssHeight) {
        overlay.style.width = `${cssWidth}px`
        overlay.style.height = `${cssHeight}px`
        lastCssWidth = cssWidth
        lastCssHeight = cssHeight
      }
      // The overwhelmingly common steady state has no active search + no hovered link.
      // When there's nothing to draw now AND nothing was drawn last frame, skip the
      // full-canvas clearRect (millions of px on the MAIN thread, ~60/sec/streaming pane).
      const mainSpan = getMainThreadSpan()
      const hasContent =
        state.searchMatchRects.length > 0 || state.hoverLink !== null || mainSpan !== null
      if (!hasContent && !hadContent) {
        return
      }
      // Clear (a prior frame's highlight/underline may now be gone) then repaint.
      ctx.clearRect(0, 0, width, height)
      for (const r of state.searchMatchRects) {
        ctx.fillStyle = r.active ? SEARCH_ACTIVE_FILL : SEARCH_MATCH_FILL
        ctx.fillRect(r.x, r.y, r.width, r.height)
      }
      const h = state.hoverLink
      paintAtermLinkUnderline(
        ctx,
        h ? { row: h.row, startCol: h.startCol, endCol: h.endCol } : null,
        getFgColor(),
        { cellWidth: state.cellWidth, cellHeight: state.cellHeight, dpr }
      )
      // Provider-link hover (main-thread detection): same underline affordance.
      paintAtermLinkUnderline(ctx, mainSpan, getFgColor(), {
        cellWidth: state.cellWidth,
        cellHeight: state.cellHeight,
        dpr
      })
      hadContent = hasContent
    },
    dispose: () => overlay.remove()
  }
}
