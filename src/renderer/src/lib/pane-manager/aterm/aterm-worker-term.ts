// The worker-backed `term`: a synchronous AtermTerminal-shaped facade over the
// single engine that lives in the render worker (aterm-render-worker). Reads come
// from the latest STATE snapshot + a rolling mirror of the visible grid; mutations
// post commands. So wireAtermPane / the controller / the facade / buffer shim / a11y
// bind to it UNCHANGED — there is NO second engine on the main thread.
//
// A few methods can't return a faithful value SYNCHRONOUSLY over the remote engine, so
// their sync return is a safe placeholder while the real result comes back out-of-band:
// serialize/serialize_scrollback return the debounced cache (the fresh value is awaitable
// via serializeAsync — a worker round-trip), encode_mouse_* return undefined (the encoded
// bytes arrive via the reply channel → PTY), selection_word/line return undefined (the
// text lands in the next snapshot + via selectionTextAsync), and search() returns an empty
// array (counts/highlights come from the snapshot + searchStateSnapshot). All of these ARE
// wired — only the synchronous return is a placeholder; every per-frame + scroll + drag +
// theme + search + cursor path is live.

import type { AtermTerminal } from './aterm_wasm.js'
import { createAtermWorkerQueryChannel } from './aterm-worker-query-channel'
import { buildAtermRowCells } from './aterm-worker-grid-cells'
import type { AtermWorkerRequest, AtermWorkerState } from './aterm-render-worker-protocol'

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
  /** Fired the moment the worker pushes a NEW side channel (OSC app-event, bell) or a
   *  title change — so the facade drains + re-emits the title RIGHT THEN instead of on
   *  the next process() chunk. Without this the prompt's final-chunk OSC 7/133/52 +
   *  title lag a command behind (or are lost if the pane closes idle), because
   *  process() only posts and the worker replies in a later task. */
  onSideChannel: (handler: () => void) => void
  /** Loader resolves a pending async query (serialize/content) by its id. */
  resolveQuery: (id: number, value: string | number | boolean | null) => void
  /** Loader feeds the worker's debounced serialized-buffer cache here; the SYNC
   *  serialize()/serialize_scrollback() return it (for the non-awaitable shutdown
   *  layout-capture). Slightly stale; the awaitable paths use serializeAsync. */
  applySerializedCache: (full: string, scrollback: string) => void
  /** Fresh full-history serialize (replayable ANSI) via a worker round-trip — for the
   *  awaitable save/snapshot/fork paths. (The sync term.serialize() can't reach
   *  off-screen history; the synchronous shutdown path is served separately.) */
  serializeAsync: (scrollbackRows?: number) => Promise<string>
  serializeScrollbackAsync: (maxRows?: number) => Promise<string>
  /** Settle every in-flight async query to null + clear timers; the loader calls this
   *  BEFORE worker.terminate() so serialize/selectionText awaiters (pty-connection
   *  save/hydrate, terminal-agent-session-fork) can't hang on a reply that never comes. */
  dispose: () => void
}

type GridRow = { text: string; wrapped: boolean; len: number; widths: string; cells?: string[] }

// One decoder for the bytes-process path (TextDecoder is stateless across decode() calls
// for whole-buffer input, so reuse it instead of allocating one per feed).
const PROCESS_TEXT_DECODER = new TextDecoder()

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
  // Fired when the worker pushes new OSC/bell or a title change, so the facade drains +
  // re-emits the title immediately (not a chunk late). See onSideChannel.
  const sideChannelListeners = new Set<() => void>()
  const notifySideChannel = (): void => sideChannelListeners.forEach((fn) => fn())
  // Fired when the worker's snapshot search count/active-index changes — the worker owns
  // the match set, so results land a frame after a posted find/next/prev; the search UI
  // subscribes (onSearchStateChange) and re-reads the snapshot-backed count then.
  const searchChangeListeners = new Set<() => void>()
  // Latest debounced serialized-buffer cache from the worker (for the sync shutdown read).
  let cachedSerialize = ''
  let cachedScrollback = ''

  // Async query round-trip (serialize / selection / link / cold content reads):
  // id-correlated promises the channel resolves from 'queryResult' messages, with a
  // per-query timeout + dispose-flush so a dropped reply can't hang an awaiter.
  const queryChannel = createAtermWorkerQueryChannel(post)

  const applyState = (next: AtermWorkerState): void => {
    const metricsChanged =
      next.cellWidth !== state.cellWidth || next.cellHeight !== state.cellHeight
    // A title set on the final pre-idle chunk would otherwise wait for the next
    // process() to be re-emitted — fire the side-channel notify so it lands now.
    const titleChanged = next.title !== state.title
    const searchChanged =
      next.searchCount !== state.searchCount || next.searchActiveIndex !== state.searchActiveIndex
    state = next
    if (metricsChanged) {
      metricsListeners.forEach((fn) => fn())
    }
    if (titleChanged) {
      notifySideChannel()
    }
    if (searchChanged) {
      searchChangeListeners.forEach((fn) => fn())
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
      row.cells = buildAtermRowCells(row.text, row.widths, state.cols)
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
      // The worker owns link detection: post this cell so the worker runs link_at →
      // the next snapshot carries hoverLink/hoverCursor (drives the loader's underline
      // overlay + cursor). Return the LATEST snapshot hover link for this cell (one
      // frame lag), which serves aterm-link-input's hover affordance + click open.
      post({ type: 'setHover', row, col })
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
      post({ type: 'process', data: PROCESS_TEXT_DECODER.decode(bytes) }),
    render: () => post({ type: 'draw' }),
    resize: (rows: number, cols: number) => post({ type: 'resize', rows, cols }),
    set_px: (px: number) => post({ type: 'setPx', px }),
    set_line_height: (scale: number) => post({ type: 'setLineHeight', lineHeight: scale }),
    set_ligatures: (on: boolean) => post({ type: 'setLigatures', on }),
    set_scrollback_limit: (lines: number) => post({ type: 'setScrollbackLimit', lines }),
    set_default_cursor_style: (param: number) => post({ type: 'setDefaultCursorStyle', param }),
    // The CSI ?997 push (if any) returns via the worker reply channel → inputSink, so
    // the sync facade method returns void (the worker drains take_response itself).
    set_color_scheme: (dark: boolean) => post({ type: 'setColorScheme', dark }),
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

    // ── methods whose SYNC return is a placeholder; the real result is out-of-band
    //    (async query / reply channel / next snapshot — see file header) ──
    selection_word: (row: number, col: number) => {
      post({ type: 'selectionWord', row, col })
      return undefined
    },
    selection_line: (row: number, col: number) => {
      post({ type: 'selectionLine', row, col })
      return undefined
    },
    // Sync serialize → the debounced cache (shutdown layout-capture can't await). The
    // awaitable save paths use serializeAsync (fresh worker round-trip) instead.
    serialize: (_scrollbackRows?: number | null) => cachedSerialize,
    serialize_scrollback: (_maxRows?: number | null) => cachedScrollback,
    // Mouse reports: post to the worker to encode (it owns the protocol); the bytes
    // arrive via the reply channel → PTY. Returns undefined — the input handler gates
    // preventDefault on the snapshot mouse-tracking flags, not on this return value.
    encode_mouse_press: (col: number, row: number, button: number, mods: number) => {
      post({ type: 'mouseEncode', kind: 'press', col, row, button, mods })
      return undefined
    },
    encode_mouse_release: (col: number, row: number, button: number, mods: number) => {
      post({ type: 'mouseEncode', kind: 'release', col, row, button, mods })
      return undefined
    },
    encode_mouse_motion: (col: number, row: number, button: number, mods: number) => {
      post({ type: 'mouseEncode', kind: 'motion', col, row, button, mods })
      return undefined
    },
    encode_mouse_wheel: (col: number, row: number, up: boolean, mods: number) => {
      post({ type: 'mouseEncode', kind: 'wheel', col, row, button: 0, mods, up })
      return undefined
    },

    // Worker-only async/clear capabilities (AtermWorkerAsyncFacade): the sync facade
    // reads lag a frame after a posted mutation, so the shared selection/link input
    // handlers use these on the worker path and fall back to the sync engine in-process.
    selectionTextAsync: queryChannel.selectionTextAsync,
    linkAtAsync: queryChannel.linkAtAsync,
    clearHover: () => post({ type: 'setHover', clear: true }),
    // The worker runs search + pushes count/active-index/rect in each snapshot; expose
    // them so the controller's search-count UI reflects real matches (term.search() can't
    // return them synchronously over the seam).
    searchStateSnapshot: () => ({
      count: state.searchCount,
      activeIndex: state.searchActiveIndex,
      activeRect: state.searchActiveRect
    }),
    // Search nav/clear run in the worker (it owns the match set), so post the commands —
    // the main-thread searchController has no matches on this path. next/prev advance the
    // worker's active match (+ scroll it into view); clear stops its highlights.
    searchNext: () => post({ type: 'searchNext' }),
    searchPrev: () => post({ type: 'searchPrev' }),
    searchClear: () => post({ type: 'searchClear' }),
    onSearchStateChange: (handler: () => void) => {
      searchChangeListeners.add(handler)
      return () => searchChangeListeners.delete(handler)
    },

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
      notifySideChannel()
    },
    pushBell: () => {
      bellPending = true
      notifySideChannel()
    },
    onReply: (handler) => void replyListeners.add(handler),
    onMetricsChange: (handler) => void metricsListeners.add(handler),
    onSideChannel: (handler) => void sideChannelListeners.add(handler),
    resolveQuery: queryChannel.resolve,
    applySerializedCache: (full, scrollback) => {
      cachedSerialize = full
      cachedScrollback = scrollback
    },
    serializeAsync: queryChannel.serializeAsync,
    serializeScrollbackAsync: queryChannel.serializeScrollbackAsync,
    dispose: () => {
      queryChannel.dispose()
      // Release the JS-side state now rather than waiting for the controller graph to be
      // GC'd: the rolling grid mirror, the listener Sets, and the (capped but multi-MB)
      // serialize cache strings.
      replyListeners.clear()
      metricsListeners.clear()
      sideChannelListeners.clear()
      searchChangeListeners.clear()
      grid.clear()
      cachedSerialize = ''
      cachedScrollback = ''
    }
  }
}
