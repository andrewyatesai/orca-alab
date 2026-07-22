import type { AtermFileLinkOpener, AtermLinkProviderSource } from './aterm-link-input'
import type { AtermSearchMarkerModel } from './aterm-search-marker-model'
import type { AtermLinkContext } from './aterm-url-link-routing'
import type { AtermRendererReplySurface } from './aterm-renderer-reply-surface'
import type { AtermThemeColors } from './aterm-theme-colors'
import type { AtermRainPulse } from '../../../../../shared/aterm-rain-signal'
import type { TerminalScrollIntentTarget } from '../terminal-scroll-intent-types'

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
  /** Shape literal Matrix Rain from one payload-free observable agent/tool event. */
  noteMatrixRainPulse: (pulse: AtermRainPulse) => void
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
  /** True while a posted find's results haven't landed yet (worker path) — the
   *  count is the previous query's, so the UI labels it "~N, searching…". */
  searchIsPending: () => boolean
  /** Scrollbar match-marker model (bounded track fractions of the retained buffer)
   *  derived from the FULL sorted match list; empty when no query/matches. */
  searchMarkerModel: () => AtermSearchMarkerModel
  /** Subscribe to search-state updates that land after the call (worker pushes
   *  count/active-index a frame later; in-process notifies per highlight update);
   *  returns a disposer. */
  onSearchStateChange: (handler: () => void) => () => void
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
  /** Late-bind the facade's registered xterm-style link providers (file paths,
   *  term_/task_ handles). Link hit-testing consults them when the engine reports
   *  no link at the hovered/clicked cell. */
  setLinkProviderSource: (source: AtermLinkProviderSource) => void
  /** Invalidate the link-input's last-hovered-cell cache so the next mousemove
   *  re-evaluates links even on an unchanged cell (pane reveal recovery). */
  resetLinkHoverCache: () => void
  /** Late-bind the pane's durable cross-pane-spill overlay identity (makePaneKey
   *  tabId:leafId). Resolved at the controller-attach edge — the tab id is not
   *  knowable earlier — and retained across context-loss rebuilds. No-op unless
   *  the wiring marked the engine spill-export capable. */
  bindSpillPaneKey: (paneKey: string) => void
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
  /** Worker path only: subscribe to a fresh side-channel push (OSC app-event, bell)
   *  so the facade drains it immediately instead of a chunk late. Unset/no-op for the
   *  in-process strategies, whose post-process() drain is already synchronous. */
  onEngineSideChannel?: (handler: () => void) => void
  /** Parse fence: resolves TRUE once the engine has parsed every process() byte
   *  fed before this call — so any auto-replies (DA/CPR) those bytes generated have
   *  already been delivered. In-process parsing is synchronous (resolves true
   *  immediately); the worker path round-trips a fence message and resolves FALSE
   *  on its timeout/dispose (the worker is merely behind, NOT parse-certified). The
   *  replay guard holds its drop window open until this resolves true, never on a
   *  time-based false — a false release could leak still-unparsed query replies. */
  settle: () => Promise<boolean>
  /** Live engine KeyboardMode bitfield (kitty flags / modifyOtherKeys / DECCKM…).
   *  Lets window-level shortcut policy stand its readline-compat rewrites down
   *  once the pane's app negotiated an enhanced key protocol. Worker path reads
   *  the snapshot mirror (≤1 frame stale — the accepted worker tradeoff). */
  keyboardModeBits: () => number
  /** Encode a host-synthesized key PRESS (engine `Modifiers` bitfield:
   *  SHIFT=1 ALT=2 CTRL=4 SUPER=8) through the pane's engine under its LIVE
   *  keyboard mode — the window-level shortcut policy's 'encodeKey' seam for
   *  chords whose DOM events can't reach the textarea encoder (Cmd chords
   *  behind the metaKey firewall, macOS Option composition). Returns null when
   *  the engine has no encoding (the caller falls back to legacy bytes). */
  encodeKeyForHost: (key: string, mods: number) => string | null
  /** Re-apply ligatures / scrollback / default cursor style from the live settings to
   *  this open pane (cheap engine setters; mirrors how theme/size live-apply). Called by
   *  applyTerminalAppearance on a settings change so a toggle takes effect immediately. */
  reapplyEngineSettings: () => void
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
  /** Explicitly size the grid (xterm resize semantics: snapshot replay, mobile-fit
   *  hold). Sets an override the container ResizeObserver respects until
   *  fitToContainer clears it, so the observer can't immediately undo it. */
  resize: (cols: number, rows: number) => void
  /** Drop any explicit resize override and refit the grid to the container (the
   *  aterm equivalent of xterm's FitAddon.fit after a snapshot replay). */
  fitToContainer: () => void
  /** Subscribe to mouse/keyboard-driven selection mutations so the facade can
   *  emit onSelectionChange without waiting for PTY output (Linux PRIMARY /
   *  copy-on-select on idle shells). Worker panes notify from the state push
   *  that carries the fresh range instead. */
  onSelectionMutation: (handler: () => void) => void
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
  /** Authorize/revoke OSC 9/99/777 desktop notifications on the engine. Fail-closed
   *  by default: until authorized the engine drops them before queueing, so
   *  takeNotifications stays empty. Synced from the user's notification settings. */
  setNotificationsAuthorized: (allowed: boolean) => void
  /** True when the app has enabled any mouse tracking mode (DECSET 1000/1002/1003
   *  etc.) — the facade maps this to xterm's mouseTrackingMode ('vt200' vs 'none'). */
  isMouseTracking: () => boolean
  /** True when DECSET 1004 (focus reporting) is active. */
  isFocusEventMode: () => boolean
  /** True when DEC mode 2031 (color-scheme update notifications) is active. */
  isColorSchemeUpdatesMode: () => boolean
  /** True when DECCKM (application cursor keys) is active — the facade maps this
   *  to xterm's modes.applicationCursorKeysMode. */
  isAppCursorMode: () => boolean
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
  /** Drain queued OSC 9/99/777 desktop notifications as a JSON string
   *  `[{id,title,body,urgency},...]`, or undefined when none are pending
   *  (matches the wasm take_notifications shape). */
  takeNotifications: () => string | undefined
  /** Current selection range in display cell coords, or null when none. */
  selectionRange: () => { startX: number; startY: number; endX: number; endY: number } | null
  /** Re-apply a character selection from a captured display-cell range (the inverse
   *  of selectionRange). Used to carry the live selection across a GPU→CPU renderer
   *  rebuild, which serializes only content, not the selection. */
  restoreSelectionRange: (range: {
    startX: number
    startY: number
    endX: number
    endY: number
  }) => void
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
  /** The facade consumer's attachCustomKeyEventHandler hook (IME suppression,
   *  interrupt handling, JIS-yen). Read per keydown; a `false` return means the
   *  consumer handled/suppressed the key, so nothing is encoded for it. */
  /** Preferred IME anchor cell when an agent CLI parks the real cursor away
   *  from its visible prompt (upstream #7061); null → the engine cursor. */
  getImeAnchor?: () => { row: number; col: number } | null
  getCustomKeyEventHandler?: () => ((event: KeyboardEvent) => boolean) | null
  /** Latest terminalCursorBlink (xterm's cursorBlink); when true the focused cursor
   *  blinks on a ~530ms timer, else it's steady-on. Defaults to true. */
  getCursorBlink?: () => boolean
  /** Base cell font size in CSS px (the user's terminalFontSize). Read live so a
   *  size change re-rasterizes the engine (set_px) via the grid reflow without a
   *  pane rebuild. Defaults to ATERM_RENDERER_FONT_PX when unset. */
  getFontPx?: () => number
  /** Cell line-height multiplier (the user's terminalLineHeight, ~1–10). Read live so
   *  a change re-derives the cell-box height (set_line_height) via the grid reflow
   *  without a pane rebuild. Defaults to 1 (engine default) when unset. */
  getLineHeight?: () => number
  /** Primary font family (the user's terminalFontFamily). Read at pane open to inject
   *  the resolved face via set_primary_font; undefined / "JetBrains Mono" keeps the
   *  bundled face. */
  getFontFamily?: () => string | undefined
  /** Numeric font weight (the user's terminalFontWeight, 100–900). Read at pane open
   *  with the family: it selects which of the family's named styles becomes the
   *  primary face, and the derived bold weight picks the real bold face
   *  (set_bold_font) when the family ships one. Unset → the shared default (500). */
  getFontWeight?: () => number | undefined
  /** Whether ligatures are enabled (resolved from terminalLigatures + the font family).
   *  Read at pane open to drive set_ligatures; the engine defaults to ON, so an unset
   *  callback keeps ligatures on. Like the font family, a change applies on new panes. */
  getLigatures?: () => boolean
  /** Scrollback history line limit (resolved from terminalScrollbackBytes). Read at
   *  pane open to drive set_scrollback_limit; unset keeps the engine's 100k default. */
  getScrollbackLines?: () => number
  /** Whether the engine may advertise/honor the Kitty keyboard protocol. Per-pane
   *  STATIC policy (local Windows ConPTY panes withhold it — several local ConPTY
   *  CLIs read the advertisement but don't decode CSI-u; SSH/macOS/Linux keep it),
   *  so it's read once at engine construction. Unset → enabled (engine default). */
  getKittyKeyboardEnabled?: () => boolean
  /** DEFAULT cursor style as a DECSCUSR param (1=blinking block … 6=steady bar),
   *  resolved from terminalCursorStyle + terminalCursorBlink. Read at pane open to drive
   *  set_default_cursor_style; unset keeps the engine default (1). Does not clobber an
   *  app's live DECSCUSR. */
  getCursorStyleParam?: () => number
  /** Latest scrollSensitivity (xterm's option of the same name): multiplies the
   *  scrollback wheel line count. Read per wheel event. Defaults to 1. */
  getScrollSensitivity?: () => number
  /** Latest fastScrollSensitivity: extra multiplier while the fast-scroll
   *  modifier (Alt) is held. Read per wheel event. Defaults to 1. */
  getFastScrollSensitivity?: () => number
  /** Latest terminalTuiScrollSensitivity: multiplies discrete wheel movement in
   *  TUIs (alt-screen arrow synthesis + mouse-report wheel forwarding). Read per
   *  wheel event. Defaults to 1. */
  getTuiScrollMultiplier?: () => number
  /** orca's PaneManagerOptions.formatLinkTooltip: maps a hovered URL (+ the
   *  default affordance hint) to a richer hover-tooltip label (e.g. localhost
   *  port worktree labels), possibly async. Read per hover; a null/undefined
   *  result keeps the default "url (modifier hint)" label. */
  formatLinkTooltip?: (
    url: string,
    openLinkHint: string
  ) => string | null | undefined | Promise<string | null | undefined>
  /** The pane's scroll-intent target — the SAME facade keyboard-handlers records
   *  intent against. Threaded so the input paths that scroll the engine directly
   *  (Shift+PageUp/Down, scrollbar thumb-drag) and the context-loss renderer
   *  rebuild can record/enforce scroll intent through the shared seam, instead of
   *  leaving a keyed remount to snap the viewport to the bottom. */
  getScrollIntentTarget?: () => TerminalScrollIntentTarget | null
}
