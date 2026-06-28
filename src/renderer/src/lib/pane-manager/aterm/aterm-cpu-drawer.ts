import { loadAterm } from './load-aterm'
import { injectTerminalFallbackFonts } from './inject-terminal-fallback-fonts'
import { seedAtermPalette, seedAtermReplyDefaults } from './aterm-theme-colors'
import { createAtermFramePainter } from './aterm-frame-painter'
import { MIN_GRID_COLS, MIN_GRID_ROWS } from './aterm-grid-size'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermDrawerBuildConfig, AtermPainterBinding } from './aterm-drawer-config'
import type { AtermTerminal } from './aterm_wasm.js'

/** A CPU draw strategy that has loaded its engine + 2d canvas and only needs the
 *  controller's late-bound painter state (search/dpr/rows) to start drawing. */
export type AtermCpuDrawerPending = {
  term: AtermTerminal
  cellWidth: number
  cellHeight: number
  /** Finish the strategy once the controller has built the search controller +
   *  per-frame getters (which depend on the engine created here). */
  bindPainter: (binding: AtermPainterBinding) => AtermDrawStrategy
}

/** CPU strategy: `aterm-wasm`'s engine rasterizes the grid on the CPU and JS
 *  `putImageData`s the RGBA frame onto a 2d canvas — the current default path,
 *  extracted verbatim from the controller. Search highlights paint on the SAME
 *  canvas (its 2d context), so no overlay is needed. This path is also the
 *  fallback when the GPU path is off or fails. */
export async function loadAtermCpuDrawer(
  config: AtermDrawerBuildConfig
): Promise<AtermCpuDrawerPending> {
  const { canvas, themeColors, fontPx } = config
  // The 2d context for the CPU framebuffer blit (a canvas can have 2d OR webgl2,
  // never both — the CPU path owns 2d).
  const ctx = canvas.getContext('2d')

  const { AtermTerminal: AtermTerminalCtor, fontBytes, memory } = await loadAterm()
  // Build at a 1x1 grid to read cell metrics, then the controller sizes the real
  // grid (mirrors the original controller construction order).
  const term: AtermTerminal = new AtermTerminalCtor(
    MIN_GRID_ROWS,
    MIN_GRID_COLS,
    fontBytes,
    fontPx,
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  // Inject the local OS CJK + colour-emoji fallback faces so non-Latin scripts
  // render real glyphs instead of .notdef tofu (JetBrains Mono is Latin-only).
  await injectTerminalFallbackFonts(term)
  // Seed the 16 ANSI palette colours from the theme so SGR-indexed cell colours
  // (ls/git/prompts) render in the user's theme, not the engine's VGA defaults.
  seedAtermPalette(term, themeColors)
  // Seed the theme's selectionForeground (null → keep the WCAG floor default).
  term.set_selection_fg(themeColors.selectionForeground ?? undefined)
  // Seed the theme's inactive (unfocused) selection bg (null → engine-derived default).
  term.set_selection_inactive_bg(themeColors.selectionInactive ?? undefined)
  const cellWidth = term.cell_width
  const cellHeight = term.cell_height
  // Seed default colours + cell pixel size so aterm answers OSC 10/11 + CSI 14t/16t.
  seedAtermReplyDefaults(term, themeColors, cellWidth, cellHeight)

  return {
    term,
    cellWidth,
    cellHeight,
    bindPainter: (binding) => {
      const drawFrame = createAtermFramePainter({
        ctx,
        canvas,
        term,
        memory,
        cellWidth,
        cellHeight,
        drawScheduler: binding.drawScheduler,
        searchController: binding.searchController,
        isDisposed: binding.isDisposed,
        getDpr: binding.getDpr,
        getRows: binding.getRows,
        getSearchMatches: binding.getSearchMatches,
        getSearchActiveIndex: binding.getSearchActiveIndex,
        takeSearchRefresh: binding.takeSearchRefresh,
        getHoveredLinkSpan: binding.getHoveredLinkSpan,
        getFgColor: binding.getFgColor
      })
      return {
        term,
        getCanvas: () => canvas,
        // The CPU painter overlays search on its own 2d canvas; no separate one.
        needsSearchOverlay: false,
        drawFrame,
        resize: (rows, cols) => term.resize(rows, cols),
        dispose: () => {
          try {
            term.free()
          } catch {
            /* ignore */
          }
        }
      }
    }
  }
}
