// The engine-side "terminal" the render worker owns: it drives the single engine
// (via EngineHandle), runs search/selection/hover/follow-bottom in the worker, drains
// the engine's side channels (reply→PTY, OSC, bell), and builds the per-frame state
// snapshot (scalars + selection + hover + search + dirty visible rows) the main side
// reads synchronously. Overlay PAINTING (search highlight, link underline) is added in
// the overlay stage; this cut renders the engine grid + selection (both engine-drawn)
// and reports search/hover so the UI stays correct.

import type { EngineHandle } from './aterm-worker-engine-build'
import { createWorkerSearch } from './aterm-worker-search'
import type {
  AtermWorkerGridRow,
  AtermWorkerState,
  AtermWorkerThemeSet
} from './aterm-render-worker-protocol'

/** Drained edge-triggered side channels to forward to main right after a chunk. */
export type WorkerSideChannels = {
  /** Engine query replies (DA/DSR/CPR/colour) → PTY, or '' when none. */
  reply: string
  /** Queued OSC app-events as JSON `[[code,payload],...]`, or undefined. */
  osc: string | undefined
  /** A BEL fired this chunk. */
  bell: boolean
}

/** Latin-1 decode the engine's reply bytes (DA/DSR/CPR/colour are ASCII); drop any
 *  byte ≥ 0x80 rather than let the UTF-8 PTY write corrupt it (parity with the
 *  main-thread aterm-reply-drain). */
function decodeReply(bytes: Uint8Array | undefined): string {
  if (!bytes || bytes.length === 0) {
    return ''
  }
  let out = ''
  for (let i = 0; i < bytes.length; i++) {
    if (bytes[i] < 0x80) {
      out += String.fromCharCode(bytes[i])
    }
  }
  return out
}

export function createWorkerTerminal(handle: EngineHandle): {
  processBytes: (data: string) => WorkerSideChannels
  render: () => void
  buildState: () => AtermWorkerState
  resize: (rows: number, cols: number) => void
  setPx: (px: number) => void
  setLineHeight: (scale: number) => void
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
  setHover: (pos: { row: number; col: number } | null) => void
  searchFind: (query: string, caseSensitive: boolean, isRegex: boolean) => void
  searchNext: () => void
  searchPrev: () => void
  searchClear: () => void
  setPrimaryFont: (bytes: Uint8Array) => void
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

  // Per-visible-row signature so buildState emits only changed rows.
  let lastRowSig: string[] = []

  const followBottomAfter = (wasAtBottom: boolean): void => {
    if (wasAtBottom && e.display_offset !== 0) {
      e.scroll_to_bottom()
    }
  }

  const buildDirtyRows = (): AtermWorkerGridRow[] => {
    const dirty: AtermWorkerGridRow[] = []
    if (lastRowSig.length !== rows) {
      lastRowSig = Array.from({ length: rows }, () => ' nope')
    }
    for (let y = 0; y < rows; y++) {
      const text = e.row_text(y) ?? ''
      const wrapped = e.row_is_wrapped(y) === true
      const len = e.row_len(y) ?? cols
      const sig = `${text} ${wrapped ? 1 : 0} ${len}`
      if (sig === lastRowSig[y]) {
        continue
      }
      lastRowSig[y] = sig
      // Per-column width digit ('2' wide lead, '1' normal); only computed for rows
      // that actually changed.
      let widths = ''
      for (let x = 0; x < cols; x++) {
        widths += e.cell_is_wide(y, x) === true ? '2' : '1'
      }
      dirty.push({ y, text, wrapped, len, widths })
    }
    return dirty
  }

  return {
    processBytes: (data) => {
      const wasAtBottom = e.display_offset === 0
      handle.process(data)
      followBottomAfter(wasAtBottom)
      search.refresh()
      return {
        reply: decodeReply(e.take_response()),
        osc: e.take_osc_events(),
        bell: e.drain_bell()
      }
    },
    render: () => handle.render(),
    buildState: () => {
      const fb = handle.framebuffer()
      const range = e.selection_range()
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
        isReady: e.cell_width > 0 && e.cell_height > 0,
        title: e.title() ?? null,
        selectionRange: range
          ? { startX: range.start_x, startY: range.start_y, endX: range.end_x, endY: range.end_y }
          : null,
        selectionText: e.selection_text() ?? '',
        hoverLink,
        hoverCursor,
        searchCount: search.count(),
        searchActiveIndex: search.activeIndex(),
        searchActiveRect: search.activeRect(),
        searchMatchRects: search.visibleRects(),
        dirtyRows: buildDirtyRows()
      }
    },
    resize: (r, c) => {
      rows = r
      cols = c
      e.resize(r, c)
      search.refresh()
    },
    setPx: (px) => e.set_px(px),
    setLineHeight: (scale) => e.set_line_height(scale),
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
      return decodeReply(bytes)
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
    dispose: () => handle.dispose()
  }
}
