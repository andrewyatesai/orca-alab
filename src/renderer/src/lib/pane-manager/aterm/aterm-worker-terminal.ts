// The engine-side "terminal" the render worker owns: it drives the single engine
// (via EngineHandle), runs search/selection/hover/follow-bottom in the worker, drains
// the engine's side channels (reply→PTY, OSC, bell), and builds the per-frame state
// snapshot (scalars + selection + hover + search + dirty visible rows) the main side
// reads synchronously. Overlay PAINTING (search highlight, link underline) is added in
// the overlay stage; this cut renders the engine grid + selection (both engine-drawn)
// and reports search/hover so the UI stays correct.

import type { EngineHandle } from './aterm-worker-engine-build'
import { createWorkerSearch } from './aterm-worker-search'
import { createAtermDirtyRowTracker } from './aterm-worker-dirty-rows'
import { createAtermWorkerEffectsTick } from './aterm-worker-effects-tick'
import { decodeMouseReport, decodeReply } from './aterm-worker-reply-decode'
import type { AtermWorkerState, AtermWorkerThemeSet } from './aterm-render-worker-protocol'

/** One pane's engine-side terminal (the shared worker hosts one per live pane). */
export type WorkerTerminal = ReturnType<typeof createWorkerTerminal>

// Scrollback cap for the debounced shutdown-cache serialize — ample for session
// restore while bounding the per-push cost + the synchronous shutdown read.
const SHUTDOWN_CACHE_ROWS = 2000

/** Drained edge-triggered side channels to forward to main right after a chunk. */
export type WorkerSideChannels = {
  /** Engine query replies (DA/DSR/CPR/colour) → PTY, or '' when none. */
  reply: string
  /** Queued OSC app-events as JSON `[[code,payload],...]`, or undefined. */
  osc: string | undefined
  /** Queued OSC 9/99/777 desktop notifications as JSON
   *  `[{id,title,body,urgency},...]`, or undefined (fail-closed until authorized). */
  notifications: string | undefined
  /** A BEL fired this chunk. */
  bell: boolean
  /** New engine KeyboardMode bits when THIS chunk changed them, else undefined.
   *  Posted immediately (not with the coalesced frame STATE) because the main
   *  thread encodes keys synchronously from its snapshot mirror: an idle kitty
   *  app that flips modes with no further output would otherwise leave the
   *  mirror stale indefinitely (unbounded wrong-encoding window). */
  keyboardModeBits: number | undefined
}

export function createWorkerTerminal(handle: EngineHandle): {
  processBytes: (data: string) => WorkerSideChannels
  render: () => void
  /** Advance the clockless effects before a frame; returns true while an effect is
   *  still animating so the scheduler keeps rAF cadence (render-only frames), and
   *  false once settled — the engine's idle-to-zero contract. */
  tickEffects: () => boolean
  /** Ms until the engine's next idle one-shot (settled-cat blink), or undefined.
   *  The scheduler arms ONE timer for it — never a spinning loop. */
  effectsIdleDeadlineMs: () => number | undefined
  /** Cross an armed idle-deadline on the injected effects clock (it advances 0 while
   *  no frames run), right before the timer-fired frame renders. */
  advanceEffectsBy: (dtMs: number) => void
  buildState: () => AtermWorkerState
  /** Cheap grid dimensions (no buildState) — lets a suspended pane skip output-frame
   *  STATE posts while still posting on a dimension change (gridSize must stay correct). */
  dimensions: () => { cols: number; rows: number; cellWidth: number; cellHeight: number }
  resize: (rows: number, cols: number) => void
  setPx: (px: number) => void
  setLineHeight: (scale: number) => void
  setLigatures: (on: boolean) => void
  setScrollbackLimit: (lines: number) => void
  setDefaultCursorStyle: (param: number) => void
  /** Push the OS color scheme; returns any queued CSI ?997 reply bytes (ASCII), '' if none. */
  setColorScheme: (dark: boolean) => string
  themeSet: (m: AtermWorkerThemeSet) => void
  setCursorBlinkPhase: (on: boolean) => void
  setCursorHollow: (hollow: boolean) => void
  scrollLines: (delta: number) => void
  scrollToBottom: () => void
  scrollToTop: () => void
  scrollToLine: (line: number) => void
  selectionStart: (row: number, col: number) => void
  selectionExtend: (row: number, col: number) => void
  selectionFinish: () => void
  selectionWord: (row: number, col: number) => void
  selectionLine: (row: number, col: number) => void
  selectionClear: () => void
  setSelectionInactive: (inactive: boolean) => void
  setSelectionInactiveBg: (bg: number | null) => void
  setClipboardWriteAuthorized: (allowed: boolean) => void
  setNotificationsAuthorized: (allowed: boolean) => void
  setHover: (pos: { row: number; col: number } | null) => void
  searchFind: (query: string, caseSensitive: boolean, isRegex: boolean) => void
  searchNext: () => void
  searchPrev: () => void
  searchClear: () => void
  setPrimaryFont: (bytes: Uint8Array) => void
  setBoldFont: (bytes: Uint8Array) => void
  mouseEncode: (
    kind: 'press' | 'release' | 'motion' | 'wheel',
    col: number,
    row: number,
    button: number,
    mods: number,
    up: boolean
  ) => string
  query: (
    kind: string,
    arg: number | undefined,
    arg2: number | undefined
  ) => string | number | boolean | null
  /** Capped serialize for the debounced shutdown cache (full + scrollback-only). */
  serializedCache: () => { full: string; scrollback: string }
  dispose: () => void
} {
  const e = handle.engine
  let rows = 0
  let cols = 0

  // Search runs in the worker (it owns the engine); the main search API posts commands
  // and reads count/active/rect from the snapshot.
  const search = createWorkerSearch(handle, () => rows)

  // Hover (link detection) for the snapshot; the underline is painted in the overlay stage.
  let hoverLink: AtermWorkerState['hoverLink'] = null
  let hoverCursor = ''
  // Last emitted selection range key, so buildState only re-materializes + clones the
  // (potentially huge) selection text when the selection actually changed. '\u0000' is a
  // sentinel no real key matches, so the first frame always emits it.
  let lastSelectionKey = '\u0000'

  // Per-visible-row change detection (emit only changed rows); state + logic extracted.
  const dirtyRowTracker = createAtermDirtyRowTracker(e)

  // Effects clock (extracted): the frame scheduler drives it per rendered frame.
  const effectsTick = createAtermWorkerEffectsTick(e)

  const followBottomAfter = (wasAtBottom: boolean): void => {
    if (wasAtBottom && e.display_offset !== 0) {
      e.scroll_to_bottom()
    }
  }

  return {
    processBytes: (data) => {
      const wasAtBottom = e.display_offset === 0
      // Mode flip detection must bracket process(): DECSET/kitty push/pop in
      // this chunk changes encoding for the very next keystroke.
      const keyboardModeBitsBefore = e.keyboard_mode_bits
      handle.process(data)
      followBottomAfter(wasAtBottom)
      search.refresh()
      const keyboardModeBitsAfter = e.keyboard_mode_bits
      return {
        reply: decodeReply(e.take_response()),
        osc: e.take_osc_events(),
        notifications: e.take_notifications(),
        bell: e.drain_bell(),
        keyboardModeBits:
          keyboardModeBitsAfter !== keyboardModeBitsBefore ? keyboardModeBitsAfter : undefined
      }
    },
    render: () => handle.render(),
    tickEffects: effectsTick.tick,
    effectsIdleDeadlineMs: effectsTick.idleDeadlineMs,
    advanceEffectsBy: effectsTick.advanceBy,
    buildState: () => {
      const fb = handle.framebuffer()
      const range = e.selection_range()
      // Re-materialize + clone the selection text ONLY when the range changed; otherwise
      // omit it (undefined) and the main side keeps the prior value. A large active
      // selection during streaming would otherwise re-clone hundreds of KB every frame.
      // The sync snapshot value is a fallback — copy-on-select uses the fresh query channel.
      const selectionKey = range
        ? `${range.start_x},${range.start_y},${range.end_x},${range.end_y}`
        : ''
      let selectionText: string | undefined
      if (selectionKey !== lastSelectionKey) {
        selectionText = e.selection_text() ?? ''
        lastSelectionKey = selectionKey
      }
      return {
        type: 'state',
        engine: handle.kind,
        width: fb.width,
        height: fb.height,
        cols,
        rows,
        cellWidth: e.cell_width,
        cellHeight: e.cell_height,
        displayOffset: e.display_offset,
        displayOriginAbsolute: e.display_origin_absolute,
        cursorX: e.cursor_x,
        cursorY: e.cursor_y,
        cursorStyle: e.cursor_style,
        baseY: e.base_y,
        isAltScreen: e.is_alt_screen,
        bracketedPasteMode: e.bracketed_paste_mode,
        isMouseTracking: e.is_mouse_tracking,
        mouseWantsMotion: e.mouse_wants_motion,
        mouseWantsAnyMotion: e.mouse_wants_any_motion,
        isFocusEventMode: e.is_focus_event_mode,
        isColorSchemeUpdatesMode: e.is_color_scheme_updates_mode,
        isAppCursorMode: e.is_app_cursor_mode,
        isAlternateScroll: e.is_alternate_scroll,
        keyboardModeBits: e.keyboard_mode_bits,
        isReady: e.cell_width > 0 && e.cell_height > 0,
        title: e.title() ?? null,
        cursorColor: e.cursor_color ?? null,
        selectionRange: range
          ? { startX: range.start_x, startY: range.start_y, endX: range.end_x, endY: range.end_y }
          : null,
        selectionText,
        hoverLink,
        hoverCursor,
        searchCount: search.count(),
        searchActiveIndex: search.activeIndex(),
        searchActiveRect: search.activeRect(),
        searchMatchRects: search.visibleRects(),
        dirtyRows: dirtyRowTracker.build(rows, cols)
      }
    },
    dimensions: () => ({ cols, rows, cellWidth: e.cell_width, cellHeight: e.cell_height }),
    resize: (r, c) => {
      rows = r
      cols = c
      e.resize(r, c)
      search.refresh()
    },
    setPx: (px) => e.set_px(px),
    setLineHeight: (scale) => e.set_line_height(scale),
    setLigatures: (on) => e.set_ligatures(on),
    setScrollbackLimit: (lines) => e.set_scrollback_limit(lines),
    setDefaultCursorStyle: (param) => e.set_default_cursor_style(param),
    setColorScheme: (dark) => {
      e.set_color_scheme(dark)
      // The CSI ?997 push (if any) is ASCII; latin1-decode like other engine replies.
      return decodeReply(e.take_response())
    },
    themeSet: (m) => {
      switch (m.op) {
        case 'theme':
          e.set_theme(m.fg, m.bg, m.cursor, m.selection)
          return
        case 'paletteColor':
          e.set_palette_color(m.index, m.r, m.g, m.b)
          return
        case 'defaultForeground':
          e.set_default_foreground(m.r, m.g, m.b)
          return
        case 'defaultBackground':
          e.set_default_background(m.r, m.g, m.b)
          return
        case 'selectionFg':
          e.set_selection_fg(m.fg ?? undefined)
          return
        case 'cellPixelSize':
          e.set_cell_pixel_size(m.width, m.height)
      }
    },
    // The main-thread cursor-blink timer (attachAtermCursorBlink) drives these as
    // commands; the engine paints the toggled phase / hollow box on the next render.
    setCursorBlinkPhase: (on) => e.set_cursor_blink_phase(on),
    setCursorHollow: (hollow) => e.set_cursor_hollow(hollow),
    scrollLines: (delta) => e.scroll_lines(delta),
    scrollToBottom: () => e.scroll_to_bottom(),
    scrollToTop: () => e.scroll_to_top(),
    scrollToLine: (line) => e.scroll_search_line_into_view(line),
    selectionStart: (row, col) => e.selection_start(row, col),
    selectionExtend: (row, col) => e.selection_extend(row, col),
    selectionFinish: () => e.selection_finish(),
    selectionWord: (row, col) => void e.selection_word(row, col),
    selectionLine: (row, col) => void e.selection_line(row, col),
    selectionClear: () => e.selection_clear(),
    setSelectionInactive: (inactive) => e.set_selection_inactive(inactive),
    setSelectionInactiveBg: (bg) => e.set_selection_inactive_bg(bg ?? undefined),
    setClipboardWriteAuthorized: (allowed) =>
      allowed ? e.authorize_clipboard_write() : e.revoke_clipboard_write(),
    setNotificationsAuthorized: (allowed) => e.authorize_notifications(allowed),
    setHover: (pos) => {
      if (!pos) {
        hoverLink = null
        hoverCursor = ''
        return
      }
      const hit = e.link_at(pos.row, pos.col)
      hoverLink = hit
        ? {
            row: pos.row,
            startCol: hit.start_col,
            endCol: hit.end_col,
            url: hit.url,
            kind: hit.kind
          }
        : null
      hoverCursor = hit ? 'pointer' : ''
    },
    searchFind: (q, cs, regex) => search.find(q, cs, regex),
    searchNext: () => search.next(),
    searchPrev: () => search.prev(),
    searchClear: () => search.clear(),
    setPrimaryFont: (bytes) => e.set_primary_font(bytes),
    setBoldFont: (bytes) => e.set_bold_font(bytes),
    mouseEncode: (kind, col, row, button, mods, up) => {
      let bytes: Uint8Array | undefined
      if (kind === 'press') {
        bytes = e.encode_mouse_press(col, row, button, mods)
      } else if (kind === 'release') {
        bytes = e.encode_mouse_release(col, row, button, mods)
      } else if (kind === 'motion') {
        bytes = e.encode_mouse_motion(col, row, button, mods)
      } else {
        bytes = e.encode_mouse_wheel(col, row, up, mods)
      }
      // Mouse reports are PTY input, not an ASCII engine reply — keep every byte
      // (legacy modes emit bytes ≥ 0x80) so the report isn't truncated.
      return decodeMouseReport(bytes)
    },
    query: (kind, arg, arg2) => {
      switch (kind) {
        case 'serialize':
          return e.serialize(arg ?? undefined)
        case 'serializeScrollback':
          return e.serialize_scrollback(arg ?? undefined)
        case 'selectionText':
          return e.selection_text() ?? ''
        case 'rowText':
          return e.row_text(arg ?? 0) ?? null
        case 'rowLen':
          return e.row_len(arg ?? 0) ?? null
        case 'rowWrapped':
          return e.row_is_wrapped(arg ?? 0) ?? null
        case 'cellText':
          return e.cell_text(arg ?? 0, arg2 ?? 0)
        case 'cellWide':
          return e.cell_is_wide(arg ?? 0, arg2 ?? 0) ?? null
        case 'linkAt': {
          const hit = e.link_at(arg ?? 0, arg2 ?? 0)
          return hit
            ? JSON.stringify({
                url: hit.url,
                kind: hit.kind,
                start_col: hit.start_col,
                end_col: hit.end_col
              })
            : null
        }
        default:
          return null
      }
    },
    // Cap the cached serialize so the debounced push + the sync shutdown read stay
    // bounded (ample scrollback for session restore; the awaitable save paths use the
    // uncapped query round-trip).
    serializedCache: () => ({
      full: e.serialize(SHUTDOWN_CACHE_ROWS),
      scrollback: e.serialize_scrollback(SHUTDOWN_CACHE_ROWS)
    }),
    dispose: () => handle.dispose()
  }
}
