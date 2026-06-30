import { paintAtermLinkUnderline } from './aterm-link-underline-overlay'
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
  getDpr: () => number
): AtermWorkerOverlay {
  const overlay = document.createElement('canvas')
  overlay.dataset.testid = 'aterm-worker-overlay' // e2e locator
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
      // Match the worker framebuffer's device-pixel size (CSS = device/dpr) so the
      // rects — already in device px from the worker — land on the right cells.
      const { width, height } = state
      if (width <= 0 || height <= 0) {
        return
      }
      if (overlay.width !== width || overlay.height !== height) {
        overlay.width = width
        overlay.height = height
      }
      // The dpr the worker rendered at (see getDpr), NOT live devicePixelRatio.
      const dpr = getDpr() || 1
      overlay.style.width = `${width / dpr}px`
      overlay.style.height = `${height / dpr}px`
      // Always clear (a prior frame's highlight/underline may now be gone).
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
    },
    dispose: () => overlay.remove()
  }
}
