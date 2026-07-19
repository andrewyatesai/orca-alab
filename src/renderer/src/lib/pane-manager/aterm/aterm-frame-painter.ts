import { paintAtermSearchHighlights } from './aterm-search-overlay'
import { paintAtermLinkUnderline, type AtermHoveredLinkSpan } from './aterm-link-underline-overlay'
import { chromeCssMargins } from './aterm-chrome-box'
import { recordAtermPresent } from './aterm-present-latency-probe'
import type { AtermSearchController, AtermSearchMatch } from './aterm-search'
import type { AtermDrawScheduler } from './aterm-draw-scheduler'
import type { AtermTerminal } from './aterm_wasm.js'

/** Everything the per-frame painter reads. dpr/rows/search state are accessed via
 *  getters because they change over the pane's life (DPI move, resize, search). */
export type AtermFramePainterDeps = {
  ctx: CanvasRenderingContext2D | null
  canvas: HTMLCanvasElement
  term: AtermTerminal
  /** The shared wasm linear memory — for the zero-copy framebuffer view. */
  memory: WebAssembly.Memory
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
  /** Theme fg (0x00RRGGBB) — the hover underline color. Read live each frame so
   *  a re-theme (updateTheme) recolors the underline without a painter rebind. */
  getFgColor: () => number
}

/** Build the draw() callback that renders one frame: re-index search (coalesced),
 *  paint the engine framebuffer, size the canvas (CSS = device/dpr so the
 *  device-pixel framebuffer maps 1:1), then overlay search highlights on top. */
export function createAtermFramePainter(deps: AtermFramePainterDeps): () => void {
  const { canvas, term, drawScheduler, searchController, isDisposed, getDpr, getRows } = deps

  // Memoized CSS box (incl. the window-chrome margins): the painter runs every
  // frame, so only touch CSSOM when the frame box / chrome actually changed.
  let lastCssW = -1
  let lastCssH = -1
  let lastChromePad = -1
  let lastChromeHead = -1

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
    // Only assign on a real size change: writing canvas.width/height (even the same
    // value) resets + reallocates the backing store every frame. putImageData below
    // overwrites the whole canvas, so skipping the no-op assign is safe.
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width
      canvas.height = height
    }
    // CSS size in logical pixels so the device-pixel framebuffer maps 1:1; reads
    // dpr live so a DPI move (M2) updates the on-screen size on the next frame.
    const dpr = getDpr()
    // Window-space effects chrome (0/0 when off): the frame grows AROUND the
    // grid, so pull the box up-left by the grid's in-frame offset — the grid
    // stays put and only the chrome overhangs. `?? 0` tolerates artifact skew.
    const chromePad = term.chrome_pad ?? 0
    const chromeHead = term.chrome_head ?? 0
    const cssW = width / dpr
    const cssH = height / dpr
    if (
      cssW !== lastCssW ||
      cssH !== lastCssH ||
      chromePad !== lastChromePad ||
      chromeHead !== lastChromeHead
    ) {
      canvas.style.width = `${cssW}px`
      canvas.style.height = `${cssH}px`
      // Written explicitly both ways so toggling chrome off restores 0px.
      const margins = chromeCssMargins(chromePad, chromeHead, dpr)
      canvas.style.marginLeft = margins.marginLeft
      canvas.style.marginTop = margins.marginTop
      lastCssW = cssW
      lastCssH = cssH
      lastChromePad = chromePad
      lastChromeHead = chromeHead
    }
    // Zero-copy blit: view the engine's framebuffer directly in wasm linear memory
    // (no copy out of wasm at all — rgba_ptr returns the byte offset). Read the ptr
    // right after render() and use it synchronously before any other engine call:
    // render/process may reallocate the buffer, and wasm memory growth detaches
    // memory.buffer, so the view is rebuilt from the CURRENT buffer every frame.
    const fbView = new Uint8ClampedArray(deps.memory.buffer, term.rgba_ptr(), width * height * 4)
    ctx.putImageData(new ImageData(fbView, width, height), 0, 0)
    // Cell metrics read from the engine each frame: set_px / set_line_height
    // re-rasterize mid-life (DPI move, live font change) and stale copies would
    // misplace the overlay rects below.
    const cellWidth = term.cell_width
    const cellHeight = term.cell_height
    // Overlays are computed in grid coords; the grid sits at (pad, pad+head)
    // inside the chrome-padded frame, so shift them onto the grid.
    ctx.save()
    ctx.translate(chromePad, chromePad + chromeHead)
    // Overlay search highlights last so they sit above the rendered glyphs.
    paintAtermSearchHighlights(ctx, deps.getSearchMatches(), deps.getSearchActiveIndex(), {
      term,
      cellWidth,
      cellHeight,
      rows: getRows()
    })
    // Then the hovered-link underline (its own affordance, above the glyphs).
    paintAtermLinkUnderline(ctx, deps.getHoveredLinkSpan(), deps.getFgColor(), {
      cellWidth,
      cellHeight,
      dpr
    })
    ctx.restore()
    // e2e latency probe: the putImageData above is the real CPU present, so
    // stamp it here — only reached on a frame that actually blitted (the early
    // returns above skip no-op coalesced frames). Flag-gated, no-op otherwise.
    recordAtermPresent()
  }
}
