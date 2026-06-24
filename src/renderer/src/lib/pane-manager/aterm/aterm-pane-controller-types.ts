import type { AtermFileLinkOpener } from './aterm-link-input'
import type { AtermLinkContext } from './aterm-url-link-routing'
import type { AtermRendererReplySurface } from './aterm-renderer-reply-surface'
import type { AtermThemeColors } from './aterm-theme-colors'

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
  /** e2e/test hook: the last mouse REPORT forwarded to the PTY (e.g. an SGR
   *  "\x1b[<0;C;RM" press), or null if none. Proves a tracked mouse event was
   *  encoded + sent without relying on shell echo under a hidden window. */
  lastMouseReport: () => string | null
  /** Serialize the buffer to replayable ANSI — the aterm-native replacement for
   *  xterm's SerializeAddon (snapshot / reattach / fork / layout-persist). Mirrors
   *  `serialize({scrollback})`: `scrollbackRows` undefined → all history, `n` → the
   *  last n rows, `0` → viewport only. */
  serialize: (scrollbackRows?: number) => string
  /** Scrollback HISTORY only (the main buffer's off-screen lines) — the only
   *  recoverable history when cold-restoring an alt-screen (vim/htop) session. */
  serializeScrollback: (maxRows?: number) => string
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
}
