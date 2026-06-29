// The worker-backed `term`: a synchronous AtermTerminal-shaped facade over the
// single engine that lives in the render worker (aterm-render-worker). Reads come
// from the latest STATE snapshot + a rolling mirror of the visible grid; mutations
// post commands. So wireAtermPane / the controller / the facade / buffer shim / a11y
// bind to it UNCHANGED — there is NO second engine on the main thread.
//
// A few methods can't be faithfully synchronous over a remote engine and are wired in
// later stages: serialize/serialize_scrollback (async query — Stage C), encode_mouse_*
// (async round-trip — Stage D), selection_word/line returning text (Stage D copy-on-
// select). They return safe placeholders here; everything the per-frame + scroll +
// selection-drag + theme + search + cursor paths need is live.

import type { AtermTerminal } from './aterm_wasm.js'
import type {
  AtermWorkerQuery,
  AtermWorkerRequest,
  AtermWorkerState
} from './aterm-render-worker-protocol'

/** The initial snapshot the loader awaits (carries the first cell metrics) before it
 *  builds the controller, so construction-time reads (cell_width/height) are real. */
export type WorkerBackedTerm = {
  /** The AtermTerminal-shaped object wireAtermPane binds to (cast; see file header). */
  term: AtermTerminal
  /** Loader feeds each worker 'state' message here to refresh the sync read surface. */
  applyState: (state: AtermWorkerState) => void
  /** Loader pushes the worker's engine query replies (DA/DSR/CPR/colour) here; the
   *  wiring subscribes via onReply → PTY. Replies are PUSHED (not pull-drained) so a
   *  CPR/DA query that produces no further output can't deadlock waiting for the next
   *  drain. */
  pushReply: (data: string) => void
  /** Loader pushes queued OSC app-events (JSON `[[code,payload],...]`); the facade
   *  pull-drains them via take_osc_events on the next chunk (non-blocking). */
  pushOsc: (eventsJson: string) => void
  /** Loader pushes a BEL; the facade pull-drains it via drain_bell. */
  pushBell: () => void
  /** Wiring subscribes to forward replies to the PTY input sink. */
  onReply: (handler: (data: string) => void) => void
  /** Wiring subscribes to re-reflow the grid when the worker re-rasterizes at a new
   *  cell size (custom font size / dpr settle / live font change apply set_px AFTER the
   *  first snapshot, so the engine's metrics arrive a frame late). */
  onMetricsChange: (handler: () => void) => void
  /** Loader resolves a pending async query (serialize/content) by its id. */
  resolveQuery: (id: number, value: string | number | boolean | null) => void
  /** Fresh full-history serialize (replayable ANSI) via a worker round-trip — for the
   *  awaitable save/snapshot/fork paths. (The sync term.serialize() can't reach
   *  off-screen history; the synchronous shutdown path is served separately.) */
  serializeAsync: (scrollbackRows?: number) => Promise<string>
  serializeScrollbackAsync: (maxRows?: number) => Promise<string>
}

type GridRow = { text: string; wrapped: boolean; len: number; widths: string; cells?: string[] }

/** Reconstruct per-column graphemes from a row's text + width digits so cell_text is
 *  served from the snapshot. Wide lead cells ('2') own the grapheme; the trailing
 *  spacer column is empty. Best-effort grapheme segmentation (matches the facade's
 *  prior 1:1 char→column assumption when Segmenter is unavailable). */
function buildCells(row: GridRow, cols: number): string[] {
  const segmenter = typeof Intl !== 'undefined' && 'Segmenter' in Intl ? new Intl.Segmenter() : null
  const graphemes = segmenter
    ? Array.from(segmenter.segment(row.text), (s) => s.segment)
    : Array.from(row.text)
  const cells: string[] = Array.from({ length: cols }, () => '')
  let col = 0
  for (const g of graphemes) {
    if (col >= cols) {
      break
    }
    cells[col] = g
    // Advance by the cell's width (2 = wide lead + spacer); default 1.
    col += row.widths[col] === '2' ? 2 : 1
  }
  return cells
}

export function createWorkerBackedTerm(deps: {
  post: (cmd: AtermWorkerRequest, transfer?: Transferable[]) => void
  initial: AtermWorkerState
}): WorkerBackedTerm {
  const { post } = deps
  let state = deps.initial
  const grid = new Map<number, GridRow>()

  // Side-channel buffers: OSC app-events + bell are pull-drained by the facade; replies
  // are pushed to subscribers (see onReply).
  let oscEvents: [number, string][] = []
  let bellPending = false
  const replyListeners = new Set<(data: string) => void>()
  const metricsListeners = new Set<() => void>()

  // Async query round-trip (serialize / cold content reads): id-correlated promises the
  // loader resolves from 'queryResult' messages. Shared infra (Stage D mouse-encode reuses it).
  let nextQueryId = 1
  const pendingQueries = new Map<number, (value: string | number | boolean | null) => void>()
  const sendQuery = (
    kind: AtermWorkerQuery['kind'],
    arg?: number,
    arg2?: number
  ): Promise<string | number | boolean | null> =>
    new Promise((resolve) => {
      const id = nextQueryId++
      pendingQueries.set(id, resolve)
      post({ type: 'query', id, kind, arg, arg2 })
    })

  const applyState = (next: AtermWorkerState): void => {
    const metricsChanged =
      next.cellWidth !== state.cellWidth || next.cellHeight !== state.cellHeight
    state = next
    if (metricsChanged) {
      metricsListeners.forEach((fn) => fn())
    }
    for (const row of next.dirtyRows) {
      grid.set(row.y, {
        text: row.text,
        wrapped: row.wrapped,
        len: row.len,
        widths: row.widths
      })
    }
    // Drop rows that scrolled out of the (possibly shrunk) viewport.
    if (grid.size > next.rows) {
      for (const y of grid.keys()) {
        if (y >= next.rows) {
          grid.delete(y)
        }
      }
    }
  }

  const rowCells = (y: number): string[] => {
    const row = grid.get(y)
    if (!row) {
      return []
    }
    if (!row.cells) {
      row.cells = buildCells(row, state.cols)
    }
    return row.cells
  }

  // The AtermTerminal-shaped surface. Reads → snapshot/grid; mutations → post.
  const term = {
    // ── scalar reads (snapshot) ──
    get cell_width() {
      return state.cellWidth
    },
    get cell_height() {
      return state.cellHeight
    },
    get width() {
      return state.width
    },
    get height() {
      return state.height
    },
    get cursor_x() {
      return state.cursorX
    },
    get cursor_y() {
      return state.cursorY
    },
    get cursor_style() {
      return state.cursorStyle
    },
    get base_y() {
      return state.baseY
    },
    get display_offset() {
      return state.displayOffset
    },
    get display_origin_absolute() {
      return state.displayOriginAbsolute
    },
    get is_alt_screen() {
      return state.isAltScreen
    },
    get bracketed_paste_mode() {
      return state.bracketedPasteMode
    },
    get is_mouse_tracking() {
      return state.isMouseTracking
    },
    get mouse_wants_motion() {
      return state.mouseWantsMotion
    },
    get mouse_wants_any_motion() {
      return state.mouseWantsAnyMotion
    },
    get is_focus_event_mode() {
      return state.isFocusEventMode
    },
    get is_color_scheme_updates_mode() {
      return state.isColorSchemeUpdatesMode
    },
    get is_app_cursor_mode() {
      return state.isAppCursorMode
    },

    // ── grid-content reads (rolling visible-grid mirror) ──
    row_text: (row: number) => grid.get(row)?.text,
    row_len: (row: number) => grid.get(row)?.len,
    row_is_wrapped: (row: number) => grid.get(row)?.wrapped,
    cell_text: (row: number, col: number) => rowCells(row)[col] ?? '',
    cell_is_wide: (row: number, col: number) => {
      const row_ = grid.get(row)
      return row_ ? row_.widths[col] === '2' : undefined
    },

    // ── selection / link / title reads (snapshot) ──
    selection_text: () => state.selectionText,
    selection_range: () => {
      const r = state.selectionRange
      return r ? { start_x: r.startX, start_y: r.startY, end_x: r.endX, end_y: r.endY } : undefined
    },
    link_at: (row: number, col: number) => {
      // Only the last-hovered link is known on main; the real hover/click paths use
      // the worker (setHover command + resolveLinkAt query, Stage D).
      const h = state.hoverLink
      return h && h.row === row && col >= h.startCol && col <= h.endCol
        ? { url: h.url, kind: h.kind, start_col: h.startCol, end_col: h.endCol }
        : undefined
    },
    title: () => state.title ?? undefined,

    // ── edge-triggered side channels: drained in the WORKER + posted as events.
    //    Replies are pushed via onReply (take_response stays empty); OSC + bell are
    //    pull-drained here from the loader-fed buffers. ──
    take_response: () => undefined,
    take_osc_events: () => {
      if (oscEvents.length === 0) {
        return undefined
      }
      const json = JSON.stringify(oscEvents)
      oscEvents = []
      return json
    },
    drain_bell: () => {
      const fired = bellPending
      bellPending = false
      return fired
    },

    // ── mutations (post commands) ──
    process_str: (s: string) => post({ type: 'process', data: s }),
    process: (bytes: Uint8Array) =>
      post({ type: 'process', data: new TextDecoder().decode(bytes) }),
    render: () => post({ type: 'draw' }),
    resize: (rows: number, cols: number) => post({ type: 'resize', rows, cols }),
    set_px: (px: number) => post({ type: 'setPx', px }),
    set_line_height: (scale: number) => post({ type: 'setLineHeight', lineHeight: scale }),
    scroll_lines: (delta: number) => post({ type: 'scrollLines', delta }),
    scroll_to_bottom: () => post({ type: 'scrollToBottom' }),
    scroll_to_top: () => post({ type: 'scrollToTop' }),
    scroll_search_line_into_view: (line: number) => post({ type: 'scrollToLine', line }),
    selection_start: (row: number, col: number) => post({ type: 'selectionStart', row, col }),
    selection_extend: (row: number, col: number) => post({ type: 'selectionExtend', row, col }),
    selection_finish: () => post({ type: 'selectionFinish' }),
    selection_clear: () => post({ type: 'selectionClear' }),
    set_selection_inactive: (inactive: boolean) => post({ type: 'setSelectionInactive', inactive }),
    set_selection_inactive_bg: (bg?: number | null) =>
      post({ type: 'setSelectionInactiveBg', bg: bg ?? null }),
    set_selection_fg: (fg?: number | null) =>
      post({ type: 'themeSet', op: 'selectionFg', fg: fg ?? null }),
    set_theme: (fg: number, bg: number, cursor: number, selection: number) =>
      post({ type: 'themeSet', op: 'theme', fg, bg, cursor, selection }),
    set_palette_color: (index: number, r: number, g: number, b: number) =>
      post({ type: 'themeSet', op: 'paletteColor', index, r, g, b }),
    set_default_foreground: (r: number, g: number, b: number) =>
      post({ type: 'themeSet', op: 'defaultForeground', r, g, b }),
    set_default_background: (r: number, g: number, b: number) =>
      post({ type: 'themeSet', op: 'defaultBackground', r, g, b }),
    set_cell_pixel_size: (width: number, height: number) =>
      post({ type: 'themeSet', op: 'cellPixelSize', width, height }),
    set_cursor_blink_phase: (on: boolean) => post({ type: 'setCursorBlinkPhase', on }),
    set_cursor_hollow: (hollow: boolean) => post({ type: 'setCursorHollow', hollow }),
    set_primary_font: (bytes: Uint8Array) => post({ type: 'setPrimaryFont', bytes }),
    authorize_clipboard_write: () => post({ type: 'setClipboardWriteAuthorized', allowed: true }),
    revoke_clipboard_write: () => post({ type: 'setClipboardWriteAuthorized', allowed: false }),
    search: (query: string, caseSensitive: boolean, isRegex?: boolean) => {
      post({ type: 'searchFind', query, caseSensitive, isRegex: isRegex ?? false })
      // Counts/highlights come back via the snapshot; the search controller reads them
      // from the controller's snapshot-backed getters (Stage D wires the search API).
      return new Uint32Array(0)
    },

    // ── placeholders wired in later stages (see file header) ──
    selection_word: (row: number, col: number) => {
      post({ type: 'selectionWord', row, col })
      return undefined
    },
    selection_line: (row: number, col: number) => {
      post({ type: 'selectionLine', row, col })
      return undefined
    },
    serialize: (_scrollbackRows?: number | null) => '',
    serialize_scrollback: (_maxRows?: number | null) => '',
    encode_mouse_press: () => undefined,
    encode_mouse_release: () => undefined,
    encode_mouse_motion: () => undefined,
    encode_mouse_wheel: () => undefined,

    free: () => post({ type: 'dispose' })
  }

  return {
    term: term as unknown as AtermTerminal,
    applyState,
    pushReply: (data) => replyListeners.forEach((fn) => fn(data)),
    pushOsc: (eventsJson) => {
      try {
        oscEvents.push(...(JSON.parse(eventsJson) as [number, string][]))
      } catch {
        /* malformed OSC payload — drop */
      }
    },
    pushBell: () => {
      bellPending = true
    },
    onReply: (handler) => void replyListeners.add(handler),
    onMetricsChange: (handler) => void metricsListeners.add(handler),
    resolveQuery: (id, value) => {
      const resolve = pendingQueries.get(id)
      if (resolve) {
        pendingQueries.delete(id)
        resolve(value)
      }
    },
    serializeAsync: async (scrollbackRows) => {
      const v = await sendQuery('serialize', scrollbackRows)
      return typeof v === 'string' ? v : ''
    },
    serializeScrollbackAsync: async (maxRows) => {
      const v = await sendQuery('serializeScrollback', maxRows)
      return typeof v === 'string' ? v : ''
    }
  }
}
