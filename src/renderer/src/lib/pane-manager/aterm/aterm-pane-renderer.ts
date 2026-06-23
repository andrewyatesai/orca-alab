import { loadAterm } from './load-aterm'
import { attachAtermTextareaInput } from './aterm-textarea-input'
import { resolveAtermThemeColors } from './aterm-theme-colors'
import { attachAtermScrollInput } from './aterm-scroll-input'
import { attachAtermSelectionInput } from './aterm-selection-input'
import { attachAtermEventReportingInput } from './aterm-event-reporting-input'
import { computeGrid, MIN_GRID_COLS, MIN_GRID_ROWS } from './aterm-grid-size'
import { attachAtermLinkInput, type AtermFileLinkOpener } from './aterm-link-input'
import {
  createAtermSearchController,
  type AtermSearchController,
  type AtermSearchMatch
} from './aterm-search'
import { createAtermDrawScheduler } from './aterm-draw-scheduler'
import { attachAtermDprTracker } from './aterm-dpr-tracker'
import { createAtermFramePainter } from './aterm-frame-painter'
import { buildAtermSearchApi } from './aterm-search-api'
import { buildAtermInputDom } from './aterm-input-dom'
import { createAtermUrlOpener, type AtermLinkContext } from './aterm-url-link-routing'
import { buildAtermRendererReplySurface, type AtermRendererReplySurface } from './aterm-renderer-reply-surface'
import { copyAtermSelectionToClipboard } from './aterm-clipboard-copy'
import { e2eConfig } from '@/lib/e2e-config'
import type { AtermTerminal } from './aterm_wasm.js'

export type { AtermLinkContext } from './aterm-url-link-routing'

// Font cell size in CSS pixels; multiplied by devicePixelRatio for the engine.
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
  /** Current selection text, if any (empty string when nothing is selected). */
  selectionText: () => string
  /** Detected link at a display cell (url + kind), or null — for hit-test/tests. */
  linkAt: (row: number, col: number) => { url: string; kind: number } | null
  /** Run an in-terminal search: highlights matches, scrolls to the nearest, and
   *  returns the match count. Empty query clears highlights. */
  findMatches: (query: string, caseSensitive: boolean) => number
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
  /** e2e/test hook: the last mouse REPORT forwarded to the PTY (e.g. an SGR
   *  "\x1b[<0;C;RM" press), or null if none. Proves a tracked mouse event was
   *  encoded + sent without relying on shell echo under a hidden window. */
  lastMouseReport: () => string | null
  dispose: () => void
}

/** Optional renderer settings the controller reads live (per-press / per-frame)
 *  so a settings change takes effect without recreating the pane. */
export type AtermPaneControllerOptions = {
  /** Latest macOptionIsMeta (xterm's option of the same name); controls whether
   *  macOS Option meta-prefixes or composes a glyph. Defaults to false. */
  getMacOptionIsMeta?: () => boolean
}

export async function createAtermPaneController(
  container: HTMLElement,
  onInput: AtermPaneInputSink,
  onResize: AtermPaneResizeSink,
  onPaste: AtermPanePasteSink,
  linkContext?: AtermLinkContext,
  controllerOptions?: AtermPaneControllerOptions
): Promise<AtermPaneController> {
  const canvas = document.createElement('canvas')
  canvas.dataset.testid = 'aterm-canvas' // e2e locator for the aterm-rendered pane
  // Fill the pane; pixelated keeps the CPU-rasterized framebuffer crisp when
  // the device-pixel canvas is scaled to CSS pixels.
  canvas.style.width = '100%'
  canvas.style.height = '100%'
  canvas.style.display = 'block'
  canvas.style.imageRendering = 'pixelated'
  canvas.style.outline = 'none'
  // Mirror xterm's DOM so the app's focus/paste/IME/clipboard logic (which keys
  // off .xterm-helper-textarea / closest('.xterm')) works unchanged. Keyboard
  // focus lives on the hidden helper textarea; the canvas keeps pixels +
  // selection-drag + wheel.
  const inputDom = buildAtermInputDom(canvas)
  container.appendChild(inputDom.wrapper)

  const ctx = canvas.getContext('2d')
  const { AtermTerminal: AtermTerminalCtor, fontBytes } = await loadAterm()

  // Mutable: the window can move to a different-DPI monitor; a matchMedia
  // resolution listener (below) re-reads it so the CSS<->device mapping and grid
  // sizing track the new dpr instead of staying baked at construction (M2).
  let dpr = window.devicePixelRatio || 1
  // Seed the renderer's default fg/bg/cursor/selection from orca's active
  // terminal theme so the canvas matches the rest of the app at pane creation.
  const themeColors = resolveAtermThemeColors()
  // E2E only: stamp the exact RGB the renderer seeds as the default bg onto THIS
  // canvas (per-pane, so multiple panes don't clobber a shared global) so the
  // theme test can assert the painted top-left pixel MATCHES the configured theme
  // background (not merely "is dark"). 0x00RRGGBB → "r,g,b".
  if (e2eConfig.exposeStore) {
    const { bg } = themeColors
    canvas.dataset.atermBg = `${(bg >> 16) & 0xff},${(bg >> 8) & 0xff},${bg & 0xff}`
  }
  // Build once at an arbitrary 1x1 grid to read the engine's cell metrics, then
  // size the real grid to the container.
  const term: AtermTerminal = new AtermTerminalCtor(
    MIN_GRID_ROWS,
    MIN_GRID_COLS,
    fontBytes,
    Math.round(ATERM_RENDERER_FONT_PX * dpr),
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  const cellWidth = term.cell_width
  const cellHeight = term.cell_height

  const inputSink = onInput
  const resizeSink = onResize
  const pasteSink = onPaste
  let disposed = false
  // Coalesces search re-index to one run per draw frame: each PTY chunk only sets
  // this flag; the actual refresh happens once in draw() (see M2 — avoids an
  // O(buffer) rebuild per chunk under heavy output).
  let searchRefreshPending = false
  const initialGrid = computeGrid(container, dpr, cellWidth, cellHeight)
  term.resize(initialGrid.rows, initialGrid.cols)
  let cols = initialGrid.cols
  let rows = initialGrid.rows

  // Search-highlight state in ABSOLUTE-row coords; converted to display rows at
  // paint time so highlights track the viewport as it scrolls.
  let searchMatches: AtermSearchMatch[] = []
  let searchActiveIndex = -1

  // Leak-safe frame scheduler (single rAF + backstop timer, both cancelled on
  // dispose). Owns the drawScheduled/timeout bookkeeping; see M4. The painter
  // (created below, once searchController exists) runs via this thunk.
  let draw: () => void = () => undefined
  const drawScheduler = createAtermDrawScheduler(() => draw())
  const scheduleDraw = (): void => {
    if (disposed) {
      return
    }
    drawScheduler.schedule()
  }

  const process = (data: string): void => {
    if (disposed) {
      return
    }
    // Follow the bottom on new output ONLY if already at the bottom; if the user
    // has scrolled up to read history, leave the viewport pinned (aterm's SCR-1
    // keeps it stable while scrollback grows).
    const wasAtBottom = term.display_offset === 0
    term.process(new TextEncoder().encode(data))
    if (wasAtBottom && term.display_offset !== 0) {
      term.scroll_to_bottom()
    }
    // New output changed buffer content but search matches are stored as absolute
    // rows; mark the active query for re-index. The refresh itself is coalesced
    // into the draw frame (M2) so heavy output doesn't trigger an O(buffer)
    // rebuild per PTY chunk — only one re-index per painted frame.
    if (searchController.hasActiveQuery()) {
      searchRefreshPending = true
    }
    scheduleDraw()
  }

  // Deps held in named objects (not inline literals) so the DPI-change listener
  // can mutate `dpr` in place; selection/scroll/link read `deps.dpr` live (M2).
  const selectionDeps = {
    canvas,
    term,
    dpr,
    cellWidth,
    cellHeight,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    onCopy: copyAtermSelectionToClipboard
  }
  const selectionInput = attachAtermSelectionInput(selectionDeps)

  const scrollDeps = {
    canvas,
    term,
    dpr,
    cellHeight,
    getRows: () => rows,
    redraw: scheduleDraw,
    isDisposed: () => disposed
  }
  const scrollInput = attachAtermScrollInput(scrollDeps)

  // Mouse + focus reporting: when a TUI enables mouse tracking (DECSET
  // 1000/1002/1003) the canvas mouse events are encoded + sent to the PTY (so
  // vim/tmux/htop respond to the mouse; selection/scroll/link defer via the
  // shared gate, Shift = user override), and with DECSET 1004 the helper
  // textarea's focus/blur sends CSI I / CSI O.
  const eventReportingInput = attachAtermEventReportingInput({
    canvas,
    textarea: inputDom.textarea,
    term,
    dpr,
    cellWidth,
    cellHeight,
    inputSink,
    isDisposed: () => disposed
  })

  // Late-bound URL link context: starts from the (optional) constructor arg and
  // can be replaced by setUrlLinkContext once the React lifecycle has the pane's
  // worktreeId + in-app-link preference (the controller is created before that
  // context exists, so threading it at construction isn't always possible).
  let activeLinkContext: AtermLinkContext | undefined = linkContext

  // Open a clicked URL via orca's opener so the in-app/system-browser preference
  // (and Shift→system-browser escape hatch) is honored just like the xterm path.
  const openUrl = createAtermUrlOpener(() => activeLinkContext)

  // Late-bound file-path opener (kind 2): the lifecycle layer supplies a closure
  // with the pane's cwd/runtime context after creation. Held in a getter so the
  // link input always sees the latest binding; null until set → kind-2 no-op.
  let fileLinkOpener: AtermFileLinkOpener | null = null

  const linkDeps = {
    canvas,
    term,
    dpr,
    cellWidth,
    cellHeight,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    openUrl,
    getFileLinkOpener: () => fileLinkOpener
  }
  const linkInput = attachAtermLinkInput(linkDeps)

  // In-terminal search: the controller owns find/next/prev state and drives the
  // highlight overlay + scroll-to-match through these renderer hooks.
  const searchController: AtermSearchController = createAtermSearchController(term, {
    setSearchHighlights: (next, activeIndex) => {
      searchMatches = next
      searchActiveIndex = activeIndex
    },
    scrollToMatch: (match) => {
      if (disposed) {
        return
      }
      term.scroll_search_line_into_view(match.line)
    },
    redraw: scheduleDraw
  })

  // One-frame painter, extracted to keep this controller under the line budget.
  // dpr/rows/search state are read via getters so a DPI move, resize, or search
  // takes effect on the next frame without re-wiring the scheduler.
  draw = createAtermFramePainter({
    ctx,
    canvas,
    term,
    cellWidth,
    cellHeight,
    drawScheduler,
    searchController,
    isDisposed: () => disposed,
    getDpr: () => dpr,
    getRows: () => rows,
    getSearchMatches: () => searchMatches,
    getSearchActiveIndex: () => searchActiveIndex,
    takeSearchRefresh: () => {
      const pending = searchRefreshPending
      searchRefreshPending = false
      return pending
    }
  })

  // Search method surface (find/next/prev/clear/count/index/rect), extracted to
  // keep this controller under the line budget.
  const searchApi = buildAtermSearchApi({
    searchController,
    term,
    cellWidth,
    cellHeight,
    isDisposed: () => disposed,
    getRows: () => rows,
    getSearchMatches: () => searchMatches,
    getSearchActiveIndex: () => searchActiveIndex
  })

  // Recompute the grid for the current container size + dpr and re-resize when it
  // changed. Shared by the ResizeObserver and the DPI-change path so both keep the
  // grid, PTY size, and CSS<->device mapping consistent (M2).
  const reflowGrid = (): void => {
    if (disposed) {
      return
    }
    const next = computeGrid(container, dpr, cellWidth, cellHeight)
    if (next.cols === cols && next.rows === rows) {
      return
    }
    cols = next.cols
    rows = next.rows
    term.resize(rows, cols)
    // Mirror the new grid to the PTY so the child re-wraps for the new size.
    resizeSink(cols, rows)
    scheduleDraw()
  }

  const resizeObserver = new ResizeObserver(reflowGrid)
  resizeObserver.observe(container)

  // Track DPI changes (window moved to a different-DPI monitor) so the grid + CSS
  // mapping follow the new dpr (M2). On change, propagate dpr to the pointer/
  // scroll/mouse hit-testers, reflow the grid, then force a redraw so the CSS size
  // (width/dpr) updates even when cols/rows stayed the same.
  const dprTracker = attachAtermDprTracker({
    getDpr: () => dpr,
    isDisposed: () => disposed,
    onDprChange: (nextDpr) => {
      dpr = nextDpr
      selectionDeps.dpr = nextDpr
      scrollDeps.dpr = nextDpr
      linkDeps.dpr = nextDpr
      eventReportingInput.setDpr(nextDpr)
      reflowGrid()
      scheduleDraw()
    }
  })

  const { textarea } = inputDom
  // Keyboard + text wiring (keydown = non-text keys, 'input'/IME = text/paste),
  // owned by a focused module so this controller stays under the line budget.
  // Cmd/Ctrl+V and primary-selection paste route through the app's existing paste
  // path (keyed off the 'xterm-helper-textarea' class), which setRangeText()s the
  // text and dispatches an InputEvent — captured by the input handler.
  const textareaInput = attachAtermTextareaInput({
    textarea,
    term,
    inputSink,
    pasteSink,
    copySelection: () => selectionInput.copySelection(),
    getMacOptionIsMeta: controllerOptions?.getMacOptionIsMeta
  })

  // Click anywhere on the canvas starts selection (canvas mousedown) AND lands
  // keyboard focus on the helper textarea so typing routes to the PTY.
  const onPointerDown = (): void => {
    textarea.focus()
  }
  canvas.addEventListener('pointerdown', onPointerDown)

  // Report the initial grid so the PTY spawns/resizes to match the canvas.
  resizeSink(cols, rows)
  scheduleDraw()

  const getGrid = (): { cols: number; rows: number } => ({ cols, rows })
  const replySurface = buildAtermRendererReplySurface({ term, cellWidth, cellHeight, themeColors, getGrid, scheduleDraw })

  return {
    process,
    displayOffset: () => term.display_offset,
    scrollLines: (delta: number) => {
      if (disposed) {
        return
      }
      term.scroll_lines(delta)
      scheduleDraw()
    },
    selectionText: () => term.selection_text() ?? '',
    linkAt: (row: number, col: number) => {
      const hit = term.link_at(row, col)
      return hit ? { url: hit.url, kind: hit.kind } : null
    },
    ...searchApi,
    setFileLinkOpener: (fn: AtermFileLinkOpener) => {
      fileLinkOpener = fn
    },
    setUrlLinkContext: (context: AtermLinkContext) => {
      activeLinkContext = context
    },
    lastMouseReport: () => eventReportingInput.lastMouseReport(),
    // Renderer-authoritative reply surface: pixel size (14t/16t), theme colors
    // (OSC 10/11), perf seam — see buildAtermRendererReplySurface.
    ...replySurface,
    dispose: () => {
      if (disposed) {
        return
      }
      disposed = true
      // Cancel any pending draw rAF/backstop so it can't fire after teardown.
      drawScheduler.dispose()
      resizeObserver.disconnect()
      // Drop the DPI-change listener (M2) so it can't fire after teardown.
      dprTracker.dispose()
      textareaInput.dispose()
      canvas.removeEventListener('pointerdown', onPointerDown)
      selectionInput.dispose()
      scrollInput.dispose()
      eventReportingInput.dispose()
      linkInput.dispose()
      inputDom.wrapper.remove()
      try {
        term.free()
      } catch {
        /* ignore */
      }
    }
  }
}
