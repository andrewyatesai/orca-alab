import type { AtermTerminal } from './aterm_wasm'
import type { AtermPaneController } from './aterm-pane-controller-types'
import type { AtermMetrics } from './aterm-grid-reflow'

/** The slice of the wasm engine this module reads. Both the CPU `AtermTerminal`
 *  and the GPU engine expose the same surface, so the union is structural. */
type EngineReads = Pick<
  AtermTerminal,
  | 'cursor_x'
  | 'cursor_y'
  | 'cursor_style'
  | 'cell_width'
  | 'cell_height'
  | 'base_y'
  | 'display_origin_absolute'
  | 'is_focus_event_mode'
  | 'is_mouse_tracking'
  | 'is_color_scheme_updates_mode'
  | 'is_app_cursor_mode'
  | 'is_alt_screen'
  | 'bracketed_paste_mode'
  | 'row_is_wrapped'
  | 'row_len'
  | 'row_text'
  | 'cell_text'
  | 'cell_is_wide'
  | 'link_at'
  | 'drain_bell'
  | 'take_osc_events'
  | 'selection_text'
  | 'selection_range'
  | 'selection_clear'
  | 'scroll_lines'
  | 'scroll_to_bottom'
  | 'scroll_to_top'
  | 'scroll_search_line_into_view'
>

/** The controller members backed directly by live engine state: buffer/grid
 *  reads the facade serves to xterm consumers (link hit-testing, IME box, scroll
 *  restore), the edge-triggered side channels (BEL / OSC events), and the
 *  scroll + selection commands. Extracted from the wiring to keep it focused. */
export type AtermEngineReadMembers = Pick<
  AtermPaneController,
  | 'cursorX'
  | 'cursorY'
  | 'cursorStyle'
  | 'cursorHidden'
  | 'isReady'
  | 'baseY'
  | 'displayOriginAbsolute'
  | 'isFocusEventMode'
  | 'isMouseTracking'
  | 'isColorSchemeUpdatesMode'
  | 'isAppCursorMode'
  | 'isAltScreen'
  | 'bracketedPasteMode'
  | 'rowIsWrapped'
  | 'rowLen'
  | 'rowText'
  | 'cellText'
  | 'cellIsWide'
  | 'cellSizeCss'
  | 'linkAt'
  | 'drainBell'
  | 'takeOscEvents'
  | 'selectionText'
  | 'selectionRange'
  | 'clearSelection'
  | 'scrollLines'
  | 'scrollToBottom'
  | 'scrollToTop'
  | 'scrollToLine'
>

export function buildAtermEngineReads(
  term: EngineReads,
  metrics: AtermMetrics,
  scheduleDraw: () => void,
  isDisposed: () => boolean
): AtermEngineReadMembers {
  // Scroll/selection commands redraw after mutating; guarded so a post-dispose
  // call is a no-op (mirrors the rest of the controller).
  const guardedDraw = (fn: () => void): void => {
    if (isDisposed()) {
      return
    }
    fn()
    scheduleDraw()
  }
  return {
    cursorX: () => term.cursor_x,
    cursorY: () => term.cursor_y,
    cursorStyle: () => term.cursor_style,
    // DECSCUSR discriminant 7 = Hidden; the engine's real stand-in for xterm's
    // renderer-internal coreService.isCursorHidden.
    cursorHidden: () => term.cursor_style === 7,
    // Real readiness: the controller's existence already implies the engine is
    // attached; cell metrics being present (>0) confirms it has produced its grid,
    // standing in for xterm's renderer-only isCursorInitialized.
    isReady: () => term.cell_width > 0 && term.cell_height > 0,
    baseY: () => term.base_y,
    displayOriginAbsolute: () => term.display_origin_absolute,
    isFocusEventMode: () => term.is_focus_event_mode,
    isMouseTracking: () => term.is_mouse_tracking,
    isColorSchemeUpdatesMode: () => term.is_color_scheme_updates_mode,
    isAppCursorMode: () => term.is_app_cursor_mode,
    isAltScreen: () => term.is_alt_screen,
    bracketedPasteMode: () => term.bracketed_paste_mode,
    rowIsWrapped: (row) => term.row_is_wrapped(row),
    rowLen: (row) => term.row_len(row),
    rowText: (row) => term.row_text(row),
    cellText: (row, col) => term.cell_text(row, col),
    cellIsWide: (row, col) => term.cell_is_wide(row, col),
    // CSS cell size = live device cell px / current dpr (xterm's css.cell). `metrics`
    // is updated in place by the grid reflow on a DPI change, so this tracks the real
    // cell size without a pane rebuild.
    cellSizeCss: () => ({
      width: term.cell_width / metrics.dpr,
      height: term.cell_height / metrics.dpr
    }),
    linkAt: (row, col) => {
      const hit = term.link_at(row, col)
      return hit ? { url: hit.url, kind: hit.kind } : null
    },
    drainBell: () => term.drain_bell(),
    takeOscEvents: () => term.take_osc_events(),
    selectionText: () => term.selection_text() ?? '',
    selectionRange: () => {
      const range = term.selection_range()
      return range
        ? { startX: range.start_x, startY: range.start_y, endX: range.end_x, endY: range.end_y }
        : null
    },
    clearSelection: () => guardedDraw(() => term.selection_clear()),
    scrollLines: (delta) => guardedDraw(() => term.scroll_lines(delta)),
    scrollToBottom: () => guardedDraw(() => term.scroll_to_bottom()),
    scrollToTop: () => guardedDraw(() => term.scroll_to_top()),
    // The engine places an ABSOLUTE line at/near the top visible row — the same
    // primitive search uses; matches xterm's scrollToLine(absolute).
    scrollToLine: (line) => guardedDraw(() => term.scroll_search_line_into_view(line))
  }
}
