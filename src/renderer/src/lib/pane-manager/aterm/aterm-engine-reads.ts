import type { AtermTerminal } from './aterm_wasm'
import type { AtermPaneController } from './aterm-pane-controller-types'

/** The slice of the wasm engine this module reads. Both the CPU `AtermTerminal`
 *  and the GPU engine expose the same surface, so the union is structural. */
type EngineReads = Pick<
  AtermTerminal,
  | 'cursor_x'
  | 'cursor_y'
  | 'base_y'
  | 'display_origin_absolute'
  | 'is_focus_event_mode'
  | 'is_color_scheme_updates_mode'
  | 'row_is_wrapped'
  | 'row_len'
  | 'row_text'
  | 'cell_text'
  | 'cell_is_wide'
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
  | 'baseY'
  | 'displayOriginAbsolute'
  | 'isFocusEventMode'
  | 'isColorSchemeUpdatesMode'
  | 'rowIsWrapped'
  | 'rowLen'
  | 'rowText'
  | 'cellText'
  | 'cellIsWide'
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
    baseY: () => term.base_y,
    displayOriginAbsolute: () => term.display_origin_absolute,
    isFocusEventMode: () => term.is_focus_event_mode,
    isColorSchemeUpdatesMode: () => term.is_color_scheme_updates_mode,
    rowIsWrapped: (row) => term.row_is_wrapped(row),
    rowLen: (row) => term.row_len(row),
    rowText: (row) => term.row_text(row),
    cellText: (row, col) => term.cell_text(row, col),
    cellIsWide: (row, col) => term.cell_is_wide(row, col),
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
