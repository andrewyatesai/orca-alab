import { paintAtermSearchHighlights } from './aterm-search-overlay'
import type { AtermSearchMatch } from './aterm-search'
import type { AtermTerminal } from './aterm_wasm.js'

/** A stacked 2d overlay canvas for the GPU path's search highlights. The grid
 *  canvas is webgl2-owned (a canvas can hold ONLY one context kind), so search
 *  highlights — which need a 2d fillRect — paint onto this SEPARATE canvas
 *  positioned exactly over the grid. Created only when the strategy is GPU. */
export type AtermSearchOverlayCanvas = {
  /** Paint the visible search highlights for the current frame; sizes itself to
   *  match the grid canvas first, then clears + redraws. */
  paint: (matches: AtermSearchMatch[], activeIndex: number) => void
  dispose: () => void
}

/** Build + insert an absolutely-positioned 2d overlay canvas over `gridCanvas`
 *  (same parent). Mirrors the grid's device-pixel + CSS sizing each paint so the
 *  highlight rects (computed in device px) land on the right cells regardless of
 *  swapchain resizes or DPI moves. */
export function createAtermSearchOverlayCanvas(
  gridCanvas: HTMLCanvasElement,
  deps: {
    term: AtermTerminal
    cellWidth: number
    cellHeight: number
    getDpr: () => number
    getRows: () => number
  }
): AtermSearchOverlayCanvas {
  const overlay = document.createElement('canvas')
  // Stack exactly over the grid canvas; transparent + click-through so it never
  // intercepts selection/scroll/link events (those stay on the grid canvas).
  overlay.style.position = 'absolute'
  overlay.style.left = '0'
  overlay.style.top = '0'
  overlay.style.pointerEvents = 'none'
  overlay.style.display = 'block'
  overlay.style.imageRendering = 'pixelated'
  const parent = gridCanvas.parentElement
  parent?.appendChild(overlay)

  const ctx = overlay.getContext('2d')

  return {
    paint: (matches, activeIndex) => {
      if (!ctx) {
        return
      }
      // Match the grid canvas's device-pixel buffer + CSS size so overlay device
      // coords align 1:1 with the grid's, and the rect math (device px) is exact.
      const width = gridCanvas.width
      const height = gridCanvas.height
      if (overlay.width !== width || overlay.height !== height) {
        overlay.width = width
        overlay.height = height
      }
      const dpr = deps.getDpr()
      overlay.style.width = `${width / dpr}px`
      overlay.style.height = `${height / dpr}px`
      // Always clear (a previous frame may have painted highlights now gone).
      ctx.clearRect(0, 0, width, height)
      paintAtermSearchHighlights(ctx, matches, activeIndex, {
        term: deps.term,
        cellWidth: deps.cellWidth,
        cellHeight: deps.cellHeight,
        rows: deps.getRows()
      })
    },
    dispose: () => {
      overlay.remove()
    }
  }
}
