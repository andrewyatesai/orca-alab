import type { AtermDrawScheduler } from './aterm-draw-scheduler'
import type { AtermSearchController, AtermSearchMatch } from './aterm-search'
import type { AtermThemeColors } from './aterm-theme-colors'
import type { AtermHoveredLinkSpan } from './aterm-link-underline-overlay'

/** What a drawer factory needs to create its engine + draw surface: the grid
 *  `<canvas>` (already in the DOM), the seed theme colors, and the cell font px
 *  (device pixels). Shared by the CPU and GPU drawer factories. */
export type AtermDrawerBuildConfig = {
  canvas: HTMLCanvasElement
  themeColors: AtermThemeColors
  /** Cell font-size in DEVICE pixels (ATERM_RENDERER_FONT_PX * dpr). */
  fontPx: number
  /** Cell line-height multiplier (the user's terminalLineHeight; 1 = engine default).
   *  Only the single-engine worker init reads it, so the FIRST off-main snapshot's cell
   *  box is right; the in-process drawers re-derive it via the wiring's set_line_height. */
  lineHeight?: number
  /** Build a FRESH grid canvas and swap it into the DOM in place of the current one,
   *  returning it. The worker path transferControlToOffscreen()'s the canvas before
   *  its first-frame race; if that fails the canvas is poisoned (getContext throws),
   *  so the in-process fallback must rebuild rather than die on the dead element.
   *  Omitted by callers that never take the worker path (the GPU/CPU drawers). */
  rebuildCanvas?: () => HTMLCanvasElement
}

/** The per-frame state a drawer's `drawFrame` reads, supplied AFTER the
 *  controller has built the search controller + getters (which depend on the
 *  engine the factory created). Both drawers consume the same binding so the
 *  controller wires search/dpr/rows once regardless of strategy. */
export type AtermPainterBinding = {
  drawScheduler: AtermDrawScheduler
  searchController: AtermSearchController
  isDisposed: () => boolean
  getDpr: () => number
  getRows: () => number
  getSearchMatches: () => AtermSearchMatch[]
  getSearchActiveIndex: () => number
  /** Whether a search re-index is queued; cleared by the painter once consumed. */
  takeSearchRefresh: () => boolean
  /** The link span under the pointer (or null) for the hover underline; both
   *  drawers paint it on their 2d overlay (CPU: grid context; GPU: stacked one). */
  getHoveredLinkSpan: () => AtermHoveredLinkSpan | null
  /** Theme fg (0x00RRGGBB) — the hover underline color. A getter so a live
   *  re-theme (updateTheme) is reflected without rebinding the painter. */
  getFgColor: () => number
  /** GPU path only: called when the WebGL2 context is lost so the controller can
   *  dispose the GPU strategy and swap to CPU. The CPU drawer ignores it. */
  onContextLoss: () => void
}
