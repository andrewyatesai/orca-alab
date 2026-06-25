import { applyAtermLiveTheme, type AtermThemeColors } from './aterm-theme-colors'
import type { AtermTerminal } from './aterm_wasm.js'

type ThemeMutatorDeps = {
  term: AtermTerminal
  /** The live theme object the engine + reply surface captured; mutated in place. */
  themeColors: AtermThemeColors
  /** Live cell metrics (mutated in place by the grid reflow on a DPI change), so
   *  re-theme re-rasterizes at the current cell size, not the construction one. */
  metrics: { cellWidth: number; cellHeight: number }
  scheduleDraw: () => void
}

/** The controller methods that re-theme / re-style the LIVE engine in place
 *  (host theme change, selection focus) without rebuilding the pane — extracted
 *  to keep the wiring file focused. */
export function buildAtermThemeMutators({
  term,
  themeColors,
  metrics,
  scheduleDraw
}: ThemeMutatorDeps): {
  updateTheme: (colors: AtermThemeColors) => void
  setSelectionInactive: (inactive: boolean) => void
  setSelectionInactiveBg: (bg: number | null) => void
} {
  return {
    // Re-theme this live engine in place (host theme change), avoiding a pane
    // rebuild that would drop scrollback. Caller (applyTerminalAppearance) only
    // iterates live panes; scheduleDraw no-ops if disposed.
    updateTheme: (colors) => {
      applyAtermLiveTheme(term, colors, metrics.cellWidth, metrics.cellHeight)
      // Mutate the shared themeColors IN PLACE (not reassign) so the live getters
      // — link-underline fg + the reply surface's OSC 10/11 color source, both of
      // which captured this object — read the new theme without a pane rebuild.
      Object.assign(themeColors, colors)
      scheduleDraw()
    },
    // Dim/undim the selection with pane focus (xterm selectionInactiveBackground).
    // The engine only repaints the inactive style while marked unfocused.
    setSelectionInactive: (inactive) => {
      term.set_selection_inactive(inactive)
      scheduleDraw()
    },
    // null → undefined: keep the engine's derived inactive-selection default.
    setSelectionInactiveBg: (bg) => {
      term.set_selection_inactive_bg(bg ?? undefined)
      scheduleDraw()
    }
  }
}
