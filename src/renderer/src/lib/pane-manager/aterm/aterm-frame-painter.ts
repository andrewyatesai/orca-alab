import { paintAtermSearchHighlights } from './aterm-search-overlay'
import {
  paintAtermLinkUnderline,
  type AtermHoveredLinkSpan
} from './aterm-link-underline-overlay'
import type { AtermSearchController, AtermSearchMatch } from './aterm-search'
import type { AtermDrawScheduler } from './aterm-draw-scheduler'
import type { AtermTerminal } from './aterm_wasm.js'

/** Everything the per-frame painter reads. dpr/rows/search state are accessed via
 *  getters because they change over the pane's life (DPI move, resize, search). */
export type AtermFramePainterDeps = {
  ctx: CanvasRenderingContext2D | null
  canvas: HTMLCanvasElement
  term: AtermTerminal
  cellWidth: number
  cellHeight: number
  drawScheduler: AtermDrawScheduler
  searchController: AtermSearchController
  isDisposed: () => boolean
  getDpr: () => number
  getRows: () => number
  getSearchMatches: () => AtermSearchMatch[]
  getSearchActiveIndex: () => number
  /** Whether a search re-index is queued; cleared by the painter once consumed. */
  takeSearchRefresh: () => boolean
  /** The link span under the pointer (or null); painted as a hover underline atop
   *  the glyphs each frame, on the SAME 2d context as the search highlights. */
  getHoveredLinkSpan: () => AtermHoveredLinkSpan | null
  /** Theme fg (0x00RRGGBB) — the hover underline color. */
  fgColor: number
}

/** Build the draw() callback that renders one frame: re-index search (coalesced),
 *  paint the engine framebuffer, size the canvas (CSS = device/dpr so the
 *  device-pixel framebuffer maps 1:1), then overlay search highlights on top. */
export function createAtermFramePainter(deps: AtermFramePainterDeps): () => void {
  const {
    canvas,
    term,
    cellWidth,
    cellHeight,
    drawScheduler,
    searchController,
    isDisposed,
    getDpr,
    getRows
  } = deps

  return (): void => {
    const ctx = deps.ctx
    if (isDisposed() || !drawScheduler.isScheduled() || !ctx) {
      return
    }
    // Consume the scheduled frame (clears the rAF/timer race's losing backstop).
    drawScheduler.consume()
    // Re-index the active search at most once per frame (coalesced from N PTY
    // chunks) so highlights track current content without a per-chunk rebuild.
    if (deps.takeSearchRefresh() && searchController.hasActiveQuery()) {
      searchController.refresh()
    }
    term.render()
    const width = term.width
    const height = term.height
    canvas.width = width
    canvas.height = height
    // CSS size in logical pixels so the device-pixel framebuffer maps 1:1; reads
    // dpr live so a DPI move (M2) updates the on-screen size on the next frame.
    const dpr = getDpr()
    canvas.style.width = `${width / dpr}px`
    canvas.style.height = `${height / dpr}px`
    ctx.putImageData(new ImageData(new Uint8ClampedArray(term.rgba()), width, height), 0, 0)
    // Overlay search highlights last so they sit above the rendered glyphs.
    paintAtermSearchHighlights(ctx, deps.getSearchMatches(), deps.getSearchActiveIndex(), {
      term,
      cellWidth,
      cellHeight,
      rows: getRows()
    })
    // Then the hovered-link underline (its own affordance, above the glyphs).
    paintAtermLinkUnderline(ctx, deps.getHoveredLinkSpan(), deps.fgColor, {
      cellWidth,
      cellHeight,
      dpr
    })
  }
}
