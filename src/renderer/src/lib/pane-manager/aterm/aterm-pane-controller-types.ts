import type { AtermFileLinkOpener } from './aterm-link-input'
import type { AtermLinkContext } from './aterm-url-link-routing'
import type { AtermRendererReplySurface } from './aterm-renderer-reply-surface'
import type { AtermThemeColors } from './aterm-theme-colors'

/** Base cell font size in CSS px; scaled by devicePixelRatio for device-px
 *  rendering. Shared home so the wiring (dpr re-rasterize) and the pane renderer
 *  (construction fontPx) agree without a circular import. */
export const ATERM_RENDERER_FONT_PX = 14

export type AtermPaneInputSink = (data: string) => void
export type AtermPaneResizeSink = (cols: number, rows: number) => void
/** Send PASTED text; wraps with bracketed-paste markers when DECSET 2004 is on. */
export type AtermPanePasteSink = (data: string) => void

// The renderer-authoritative reply surface (pixelSize / themeColors / e2e
// benchmarkRender) is mixed in so CSI 14t/16t + OSC 10/11 answers live with it.
export type AtermPaneController = AtermRendererReplySurface & {
  /** Feed PTY/replay output bytes; coalesces draws into one rAF frame. */
  process: (data: string) => void
  /** Lines the viewport is scrolled up from the live bottom (0 = at bottom). */
  displayOffset: () => number
  /** Scroll scrollback (positive = older); redraws. Mirrors the wheel path. */
  scrollLines: (delta: number) => void
  /** Snap the viewport to the live bottom (latest output); redraws. */
  scrollToBottom: () => void
  /** Snap the viewport to the oldest retained scrollback line; redraws. */
  scrollToTop: () => void
  /** Scroll so the ABSOLUTE buffer line `line` is at/near the top visible row
   *  (xterm scrollToLine semantics); redraws. */
  scrollToLine: (line: number) => void
  /** Current selection text, if any (empty string when nothing is selected). */
  selectionText: () => string
  /** Detected link at a display cell (url + kind), or null — for hit-test/tests. */
  linkAt: (row: number, col: number) => { url: string; kind: number } | null
  /** Run an in-terminal search: highlights matches, scrolls to the nearest, and
   *  returns the match count. Empty query clears highlights. `isRegex` compiles the
   *  query as a regex (invalid pattern → 0 matches). */
  findMatches: (query: string, caseSensitive: boolean, isRegex: boolean) => number
  /** Move the active highlight to the next match (wraps); scrolls into view. */
  findNextMatch: () => void
  /** Move the active highlight to the previous match (wraps); scrolls into view. */
  findPreviousMatch: () => void
  /** Drop all search highlights (close / empty query). */
  clearSearch: () => void
  /** Total matches for the current query (0 when none / no query). */
  searchMatchCount: () => number
  /** 1-based active match index, or 0 when there are no matches. */
  searchActiveMatchIndex: () => number
  /** Device-pixel rect of the active match's highlight on the canvas (the exact
   *  cell band the overlay paints), or null when there is no on-screen active
   *  match. Mirrors paintAtermSearchHighlights' mapping; used to verify the
   *  highlight lands on the match cells. */
  searchActiveMatchRect: () => { x: number; y: number; width: number; height: number } | null
  /** Late-bind the file-path link opener (kind 2). The lifecycle layer supplies a
   *  closure that resolves the raw path against the pane's cwd/runtime and opens
   *  it; until set, kind-2 clicks are a no-op (cursor still shows pointer). */
  setFileLinkOpener: (fn: AtermFileLinkOpener) => void
  /** Late-bind the URL link context (worktreeId + in-app-link preference) so URL
   *  clicks honor orca's open-links-in-app preference once the lifecycle has it. */
  setUrlLinkContext: (context: AtermLinkContext) => void
  /** Re-theme the live engine in place (host theme change) without rebuilding the
   *  pane — updates default fg/bg/cursor/selection + ANSI palette + reply defaults
   *  and redraws, preserving scrollback. */
  updateTheme: (colors: AtermThemeColors) => void
  /** Mark the pane focused/unfocused so a selection dims while the pane is
   *  unfocused (xterm's selectionInactiveBackground behavior). Wired to the same
   *  focus/blur transitions that drive the hollow cursor. */
  setSelectionInactive: (inactive: boolean) => void
  /** Set the inactive (unfocused) selection background (0x00RRGGBB), or null to
   *  let the engine derive it from the active selection bg blended toward the bg. */
  setSelectionInactiveBg: (bg: number | null) => void
  /** Schedule a canvas redraw (coalesced into one frame). Lets the output
   *  scheduler repaint the engine's mirrored state after a callback-only
   *  __schedulerWrite, which feeds no bytes and so schedules no draw of its own. */
  scheduleDraw: () => void
  /** Which draw path this pane is on: 'gpu' = the WebGL2 drawer, 'cpu' = the 2d
   *  drawer (default + the GPU→CPU context-loss fallback). Sourced from the loaded
   *  strategy's `kind`; flips to 'cpu' after a context-loss swap. */
  rendererKind: () => 'gpu' | 'cpu'
  /** The acquired WebGL adapter/backend string on the GPU path, else null (CPU). */
  adapterInfo: () => string | null
  /** Pause/resume this pane's draw scheduling (hidden-pane gating). While
   *  suspended the engine still ingests PTY bytes (state stays current) but no
   *  frame is painted; resume repaints the latest state if a draw was wanted. */
  setDrawSuspended: (suspended: boolean) => void
  /** e2e/test hook: the last mouse REPORT forwarded to the PTY (e.g. an SGR
   *  "\x1b[<0;C;RM" press), or null if none. Proves a tracked mouse event was
   *  encoded + sent without relying on shell echo under a hidden window. */
  lastMouseReport: () => string | null
  /** Serialize the buffer to replayable ANSI — the aterm-native replacement for
   *  xterm's SerializeAddon (snapshot / reattach / fork / layout-persist). Mirrors
   *  `serialize({scrollback})`: `scrollbackRows` undefined → all history, `n` → the
   *  last n rows, `0` → viewport only. */
  serialize: (scrollbackRows?: number) => string
  /** FRESH serialize, awaitable. Identical to `serialize` for the in-process engine
   *  (resolves synchronously), but on the single-engine WORKER path it round-trips to
   *  the worker so the result reflects off-screen history + the latest output (the sync
   *  `serialize` there can only return a cached/empty blob). Save/snapshot/fork use this. */
  serializeAsync: (scrollbackRows?: number) => Promise<string>
  /** Scrollback HISTORY only (the main buffer's off-screen lines) — the only
   *  recoverable history when cold-restoring an alt-screen (vim/htop) session. */
  serializeScrollback: (maxRows?: number) => string
  /** Awaitable scrollback-history serialize (worker round-trip on the worker path). */
  serializeScrollbackAsync: (maxRows?: number) => Promise<string>
  /** Window title (OSC 0/2), or null when unset. */
  title: () => string | null
  /** Subscribe to OSC 0/2 title changes (re-homed off xterm's onTitleChange).
   *  Returns an xterm-compatible disposable. */
  onTitleChange: (handler: (title: string) => void) => { dispose: () => void }
  /** Current grid size (cols × rows) — for snapshot metadata without xterm. */
  gridSize: () => { cols: number; rows: number }
  /** True when the alternate screen (TUI) is active — snapshot hydration uses this
   *  to avoid bleeding normal-buffer scrollback into a mid-TUI seed. */
  isAltScreen: () => boolean
  /** True when bracketed-paste mode (DECSET 2004) is active — the paste sink wraps
   *  pasted text in ESC[200~..ESC[201~ itself instead of routing through xterm.paste. */
  bracketedPasteMode: () => boolean
  /** Authorize/revoke OSC 52 clipboard *write* on the engine. The engine is
   *  fail-closed by default and won't queue OSC 52 set events (take_osc_events)
   *  until authorized; the host still gates the actual clipboard write on the
   *  user's terminalAllowOsc52Clipboard setting (defense in depth). */
  setClipboardWriteAuthorized: (allowed: boolean) => void
  /** True when the app has enabled any mouse tracking mode (DECSET 1000/1002/1003
   *  etc.) — the facade maps this to xterm's mouseTrackingMode ('vt200' vs 'none'). */
  isMouseTracking: () => boolean
  /** True when DECSET 1004 (focus reporting) is active. */
  isFocusEventMode: () => boolean
  /** True when DEC mode 2031 (color-scheme update notifications) is active. */
  isColorSchemeUpdatesMode: () => boolean
  /** Display-relative cursor column (0-based). */
  cursorX: () => number
  /** Display-relative cursor row (0-based, top of viewport). */
  cursorY: () => number
  /** Absolute row index of the live bottom line (xterm buffer.active.baseY). */
  baseY: () => number
  /** Absolute row index of the TOP visible line (`base_y - display_offset`). */
  displayOriginAbsolute: () => number
  /** Soft-wrap flag for a visible display `row`, or undefined when out of range. */
  rowIsWrapped: (row: number) => boolean | undefined
  /** Logical length of a visible display `row` (last non-empty cell + 1), or
   *  undefined when out of range. */
  rowLen: (row: number) => number | undefined
  /** Scroll-correct text of a display `row` (display_offset-aware), or undefined
   *  when out of range. */
  rowText: (row: number) => string | undefined
  /** Grapheme text at a visible display cell (`row`/`col`); empty for blanks. */
  cellText: (row: number, col: number) => string
  /** Whether a visible display cell is a wide (double-width) char; undefined when
   *  out of range. */
  cellIsWide: (row: number, col: number) => boolean | undefined
  /** Active DECSCUSR cursor-style discriminant (1-6 = visible styles, 7 = Hidden,
   *  8 = HollowBlock) — the engine's real cursor_style. Replaces reads of xterm's
   *  renderer-internal coreService for cursor introspection in tests. */
  cursorStyle: () => number
  /** True iff the cursor is hidden (cursor_style === 7). The honest aterm
   *  equivalent of xterm's `_core.coreService.isCursorHidden`. */
  cursorHidden: () => boolean
  /** Real CSS cell size in px = device cell px (term.cell_width/height) / current
   *  devicePixelRatio. Replaces xterm's `_renderService.dimensions.css.cell`. */
  cellSizeCss: () => { width: number; height: number }
  /** True once the live engine is attached + has produced its grid metrics — the
   *  aterm-native stand-in for xterm's renderer-only `isCursorInitialized`. The
   *  controller's existence already implies attachment; this confirms real cell
   *  metrics are present (cell_width/height > 0). */
  isReady: () => boolean
  /** Drain the edge-triggered BEL flag: true if a BEL fired since the last call. */
  drainBell: () => boolean
  /** Drain queued OSC app-events as a JSON string `[[code,payload],...]`, or
   *  undefined when none are pending (matches the wasm take_osc_events shape). */
  takeOscEvents: () => string | undefined
  /** Current selection range in display cell coords, or null when none. */
  selectionRange: () => { startX: number; startY: number; endX: number; endY: number } | null
  /** Clear the active selection (removes highlight). */
  clearSelection: () => void
  /** The aterm `.xterm` DOM wrapper (mirrors xterm's element for hit-testing). */
  element: HTMLElement
  /** The aterm helper textarea (mirrors xterm's textarea for focus/IME). */
  textarea: HTMLTextAreaElement
  dispose: () => void
}

/** Optional renderer settings the controller reads live (per-press / per-frame)
 *  so a settings change takes effect without recreating the pane. */
export type AtermPaneControllerOptions = {
  /** Latest macOptionIsMeta (xterm's option of the same name); controls whether
   *  macOS Option meta-prefixes or composes a glyph. Defaults to false. */
  getMacOptionIsMeta?: () => boolean
  /** Latest terminalClipboardOnSelect (xterm's copyOnSelect); when true, drag /
   *  double / triple-click auto-copy the selection. Defaults to false. */
  getCopyOnSelect?: () => boolean
  /** Latest terminalCursorBlink (xterm's cursorBlink); when true the focused cursor
   *  blinks on a ~530ms timer, else it's steady-on. Defaults to true. */
  getCursorBlink?: () => boolean
  /** Base cell font size in CSS px (the user's terminalFontSize). Read live so a
   *  size change re-rasterizes the engine (set_px) via the grid reflow without a
   *  pane rebuild. Defaults to ATERM_RENDERER_FONT_PX when unset. */
  getFontPx?: () => number
  /** Cell line-height multiplier (the user's terminalLineHeight, ~1–3). Read live so
   *  a change re-derives the cell-box height (set_line_height) via the grid reflow
   *  without a pane rebuild. Defaults to 1 (engine default) when unset. */
  getLineHeight?: () => number
  /** Primary font family (the user's terminalFontFamily). Read at pane open to inject
   *  the resolved face via set_primary_font; undefined / "JetBrains Mono" keeps the
   *  bundled face. */
  getFontFamily?: () => string | undefined
}
