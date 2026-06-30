import { attachAtermTextareaInput } from './aterm-textarea-input'
import { attachAtermCursorBlink } from './aterm-cursor-blink'
import { buildAtermThemeMutators } from './aterm-controller-theme-mutators'
import { attachAtermPointerInputs } from './aterm-pointer-input-bundle'
import { computeGrid } from './aterm-grid-size'
import type { AtermFileLinkOpener } from './aterm-link-input'
import { createAtermSearchController, type AtermSearchMatch } from './aterm-search'
import { createAtermDrawScheduler } from './aterm-draw-scheduler'
import { buildAtermSearchApi } from './aterm-search-api'
import type { AtermLinkContext } from './aterm-url-link-routing'
import { buildAtermRendererReplySurface } from './aterm-renderer-reply-surface'
import { createAtermSearchOverlayCanvas } from './aterm-search-overlay-canvas'
import { createAtermA11yMirror } from './aterm-a11y-mirror'
import { buildAtermEngineReads } from './aterm-engine-reads'
import { wireWorkerStrategyHooks } from './aterm-worker-strategy-hookup'
import { buildAtermSerializeMembers } from './aterm-serialize-members'
import { createAtermTitleChannel } from './aterm-title-channel'
import { createAtermProcessPump } from './aterm-process-pump'
import { attachAtermGridReflow, type AtermMetrics } from './aterm-grid-reflow'
import { createAtermPanePresenter } from './aterm-pane-present'
import { applyTerminalPrimaryFont } from './inject-terminal-primary-font'
import { attachAtermCanvasFocus } from './aterm-canvas-focus'
import { applyAtermEngineSettings } from './aterm-engine-settings-apply'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermPendingStrategy } from './aterm-strategy-select'
import type { AtermThemeColors } from './aterm-theme-colors'
import type {
  AtermPaneController,
  AtermPaneInputSink,
  AtermPanePasteSink,
  AtermPaneResizeSink,
  AtermPaneControllerOptions
} from './aterm-pane-controller-types'
import { createAtermControllerOptionReaders } from './aterm-controller-option-readers'

/** Everything the wiring needs to turn a loaded strategy into a live pane. */
export type AtermPaneWiringConfig = {
  pending: AtermPendingStrategy
  canvas: HTMLCanvasElement
  container: HTMLElement
  /** The `.xterm` DOM wrapper (mirrors xterm's element node). */
  element: HTMLElement
  textarea: HTMLTextAreaElement
  /** Off-screen ARIA live region the draw path mirrors grid text into (a11y). */
  liveRegion: HTMLElement
  themeColors: AtermThemeColors
  inputSink: AtermPaneInputSink
  resizeSink: AtermPaneResizeSink
  pasteSink: AtermPanePasteSink
  linkContext?: AtermLinkContext
  controllerOptions?: AtermPaneControllerOptions
  /** Late-bound bindings shared across a context-loss rebuild (so the file-path/
   *  URL openers set on the old controller carry over to the CPU one). */
  shared: AtermSharedLateBindings
  /** GPU path only: invoked when the WebGL2 context is lost so the controller can
   *  swap this wiring out for a CPU one (mirrors terminal-webgl-auto-policy). */
  onContextLoss: () => void
}

/** Late-bound openers that survive a GPU→CPU context-loss rebuild. */
export type AtermSharedLateBindings = {
  fileLinkOpener: AtermFileLinkOpener | null
  activeLinkContext: AtermLinkContext | undefined
}

/** A wired, drawing pane: its public controller surface plus a teardown that
 *  drops only THIS wiring (engine + handlers + overlay) — used both by the
 *  controller's dispose and by a context-loss rebuild that swaps strategies. */
export type AtermWiredPane = {
  controller: AtermPaneController
  strategy: AtermDrawStrategy
  /** Tear down handlers + overlay + the strategy (engine/canvas context). */
  teardown: () => void
}

/** Wire a loaded strategy into a full pane: scheduler, search, every input
 *  handler, the reply surface, the (GPU-only) search overlay, and the resize/DPI
 *  observers. Returns the public controller surface. All input handlers bind to
 *  `pending.term`, which exposes the SAME state surface for CPU and GPU. */
export function wireAtermPane(config: AtermPaneWiringConfig): AtermWiredPane {
  const { pending, canvas, container, element, textarea, liveRegion, themeColors, shared } = config
  const { inputSink, resizeSink, pasteSink, controllerOptions } = config
  const term = pending.term
  // Live settings readers (font size / line-height / family / ligatures / scrollback /
  // cursor), each read on demand so a change applies without a pane rebuild. Font px /
  // line-height / family are read inline here; the engine-settings applier consumes the
  // rest off `readers`.
  const readers = createAtermControllerOptionReaders(controllerOptions)
  const { getFontPx, getLineHeight, getFontFamily } = readers
  const initialDpr = window.devicePixelRatio || 1
  // `pending` was rasterized at the dpr captured when the strategy STARTED loading;
  // the async load (GPU init can take seconds) gives the window time to settle to a
  // different dpr (e.g. a headless window born at 2 settling to 1), which would leave
  // cell metrics frozen at the load-time dpr → wrong column count. Re-rasterize to the
  // live dpr now (set_px is a no-op when unchanged) so metrics + dpr agree from frame 1.
  term.set_px(Math.round(getFontPx() * initialDpr))
  // Apply the user's line-height before reading cell metrics so the grid is sized to
  // the real (scaled) cell height from frame 1; set_px re-applies it on later changes.
  term.set_line_height(getLineHeight())
  // Mutable metrics shared with the input-handler deps: a later host DPI change
  // re-rasterizes the engine (term.set_px) and updates these in place via the grid
  // reflow, so the grid + overlays resize instead of freezing at construction dpr.
  const metrics: AtermMetrics = {
    dpr: initialDpr,
    cellWidth: term.cell_width,
    cellHeight: term.cell_height
  }
  let disposed = false
  let searchRefreshPending = false

  let { cols, rows } = computeGrid(container, metrics.dpr, metrics.cellWidth, metrics.cellHeight)

  let searchMatches: AtermSearchMatch[] = []
  let searchActiveIndex = -1

  // draw (rAF) + presentNow (interactive fast path) are assigned from the presenter
  // below, once the strategy + grid reflow exist. The drawScheduler + process pump
  // capture them by closure and only invoke them at runtime (after wiring completes).
  let draw: () => void = () => undefined
  let presentNow: () => void = () => undefined
  const drawScheduler = createAtermDrawScheduler(() => draw())
  const scheduleDraw = (): void => {
    if (!disposed) {
      drawScheduler.schedule()
    }
  }

  const titleChannel = createAtermTitleChannel(term)

  const process = createAtermProcessPump({
    term,
    inputSink,
    isDisposed: () => disposed,
    emitTitleIfChanged: titleChannel.emitIfChanged,
    hasActiveSearchQuery: () => searchController.hasActiveQuery(),
    markSearchRefresh: () => {
      searchRefreshPending = true
    },
    // Present a keystroke echo immediately (coalesced to once per frame) instead of
    // waiting a full rAF — see presentNow. Bulk output still coalesces onto rAF.
    scheduleDraw: () => presentNow()
  })

  const { selectionInput, scrollInput, eventReportingInput, linkInput, syncDpr } =
    attachAtermPointerInputs({
      canvas,
      textarea,
      term,
      metrics,
      inputSink,
      controllerOptions,
      shared,
      getRows: () => rows,
      scheduleDraw,
      isDisposed: () => disposed
    })

  const searchController = createAtermSearchController(term, {
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

  // Bind the strategy's painter now that search + getters exist (they depend on
  // the engine the strategy created). The GPU strategy forwards context loss to
  // the controller (config.onContextLoss) so it can swap to CPU; CPU ignores it.
  const strategy = pending.bindPainter({
    drawScheduler,
    searchController,
    isDisposed: () => disposed,
    getDpr: () => metrics.dpr,
    getRows: () => rows,
    getSearchMatches: () => searchMatches,
    getSearchActiveIndex: () => searchActiveIndex,
    takeSearchRefresh: () => {
      const pendingRefresh = searchRefreshPending
      searchRefreshPending = false
      return pendingRefresh
    },
    getHoveredLinkSpan: () => linkInput.hoveredSpan(),
    getFgColor: () => themeColors.fg,
    onContextLoss: () => config.onContextLoss()
  })

  const searchOverlay = strategy.needsSearchOverlay
    ? createAtermSearchOverlayCanvas(canvas, {
        term,
        cellWidth: metrics.cellWidth,
        cellHeight: metrics.cellHeight,
        getDpr: () => metrics.dpr,
        getRows: () => rows,
        getHoveredLinkSpan: () => linkInput.hoveredSpan(),
        getFgColor: () => themeColors.fg
      })
    : null

  // Mirror the engine's visible rows into the off-screen ARIA live region so
  // screen readers can read output (the canvas is opaque to them). Reads the
  // engine, not the canvas, so one mirror covers both the CPU + GPU draw paths.
  const a11yMirror = createAtermA11yMirror({
    liveRegion,
    term,
    getRows: () => rows,
    isAltScreen: () => term.is_alt_screen,
    isDisposed: () => disposed
  })

  // draw + presentNow are wired from the presenter just after the grid reflow exists
  // (it's one of the presenter's deps). See below.

  const searchApi = buildAtermSearchApi({
    searchController,
    term,
    cellWidth: metrics.cellWidth,
    cellHeight: metrics.cellHeight,
    isDisposed: () => disposed,
    getRows: () => rows,
    getSearchMatches: () => searchMatches,
    getSearchActiveIndex: () => searchActiveIndex
  })

  // Size the real grid + report it so the PTY matches the canvas.
  strategy.resize(rows, cols)

  const getGrid = (): { cols: number; rows: number } => ({ cols, rows })
  const gridReflow = attachAtermGridReflow({
    term,
    container,
    metrics,
    getFontPx,
    getLineHeight,
    getGrid,
    // Worker path (onMetricsChange present): cell metrics land a frame after set_px, so
    // defer the grid commit to the worker's metrics push instead of the stale snapshot.
    // In-process set_px is synchronous (no hook) -> commit immediately (unchanged).
    asyncMetrics: strategy.onMetricsChange !== undefined,
    setGrid: (nextCols, nextRows) => {
      cols = nextCols
      rows = nextRows
      strategy.resize(rows, cols)
      resizeSink(cols, rows)
    },
    isDisposed: () => disposed,
    // Refresh the pointer/scroll/link handlers' cached metrics after a DPR change.
    syncDependents: syncDpr,
    scheduleDraw
  })

  // Worker path: forward engine query replies to the PTY + re-reflow on worker
  // re-rasterization. No-op for the in-process CPU/GPU strategies.
  wireWorkerStrategyHooks({
    strategy,
    term,
    metrics,
    inputSink,
    forceReflow: () => gridReflow.forceReflow(),
    emitTitleIfChanged: titleChannel.emitIfChanged,
    isDisposed: () => disposed
  })

  // Wire the paint path now that the strategy + grid reflow exist: the rAF draw and
  // the interactive presentNow fast path share one presenter.
  const presenter = createAtermPanePresenter({
    strategy,
    searchOverlay,
    a11yMirror,
    gridReflow,
    drawScheduler,
    scheduleDraw,
    isDisposed: () => disposed,
    getSearchMatches: () => searchMatches,
    getSearchActiveIndex: () => searchActiveIndex
  })
  draw = presenter.draw
  presentNow = presenter.presentNow

  // Honor the user's terminalFontFamily: the engine starts on the bundled JetBrains
  // Mono, then swaps in the host-resolved custom face + reflows once its bytes load
  // (async; a bundled/unresolvable family is a no-op). New panes pick up a changed
  // family; a live change applies on the next opened terminal.
  void applyTerminalPrimaryFont(term, getFontFamily()).then((applied) => {
    if (applied && !disposed) {
      gridReflow.forceReflow()
    }
  })

  const textareaInput = attachAtermTextareaInput({
    textarea,
    term,
    inputSink,
    pasteSink,
    copySelection: () => selectionInput.copySelection(),
    getMacOptionIsMeta: controllerOptions?.getMacOptionIsMeta
  })

  // Blink the cursor (focused) + draw it hollow (unfocused); the engine paints the
  // cursor but has no timer/focus model of its own.
  const cursorBlink = attachAtermCursorBlink({
    term,
    textarea,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    getCursorBlink: controllerOptions?.getCursorBlink
  })

  // Focus the helper textarea on canvas click (the canvas isn't focusable).
  const canvasFocus = attachAtermCanvasFocus(canvas, textarea)

  // Apply the user's fixed terminal settings (ligatures, scrollback depth, default cursor
  // shape) to the freshly built engine + keep the OS color scheme synced (DEC 2031 /
  // DSR 996). Defaults match the engine's own, so unset options are no-ops.
  const engineSettings = applyAtermEngineSettings({
    term,
    readers,
    inputSink,
    isDisposed: () => disposed
  })

  resizeSink(cols, rows)
  scheduleDraw()

  const replySurface = buildAtermRendererReplySurface({
    term,
    cellWidth: metrics.cellWidth,
    cellHeight: metrics.cellHeight,
    themeColors,
    getGrid,
    scheduleDraw
  })

  const teardown = (): void => {
    if (disposed) {
      return
    }
    disposed = true
    drawScheduler.dispose()
    a11yMirror.dispose()
    gridReflow.dispose()
    textareaInput.dispose()
    cursorBlink.dispose()
    canvasFocus.dispose()
    engineSettings.dispose()
    selectionInput.dispose()
    scrollInput.dispose()
    eventReportingInput.dispose()
    linkInput.dispose()
    searchOverlay?.dispose()
    strategy.dispose()
  }

  const controller: AtermPaneController = {
    process,
    displayOffset: () => term.display_offset,
    // Buffer/grid reads (incl. cellSizeCss + linkAt) + scroll/selection commands (live
    // engine state); extracted to keep this file focused.
    ...buildAtermEngineReads(term, metrics, scheduleDraw, () => disposed),
    ...searchApi,
    setFileLinkOpener: (fn: AtermFileLinkOpener) => void (shared.fileLinkOpener = fn),
    setUrlLinkContext: (context: AtermLinkContext) => void (shared.activeLinkContext = context),
    lastMouseReport: () => eventReportingInput.lastMouseReport(),
    // aterm-native serialize (replaces xterm's SerializeAddon): sync (engine / worker
    // cached blob) + awaitable (worker round-trip for fresh history). undefined → all.
    ...buildAtermSerializeMembers(term, strategy),
    title: titleChannel.title,
    onTitleChange: titleChannel.onTitleChange,
    gridSize: () => getGrid(),
    isAltScreen: () => term.is_alt_screen,
    bracketedPasteMode: () => term.bracketed_paste_mode,
    // Toggle the engine's fail-closed OSC 52 write gate so it queues OSC 52 set
    // events for the facade to drain; the host still enforces the user setting.
    setClipboardWriteAuthorized: (allowed: boolean) =>
      allowed ? term.authorize_clipboard_write() : term.revoke_clipboard_write(),
    element,
    textarea,
    // Live re-theme + selection-focus mutators (re-style the engine in place, no
    // pane rebuild). `metrics` is passed by reference so re-theme reads the
    // current cell size after a DPI change, not the construction one.
    ...buildAtermThemeMutators({ term, themeColors, metrics, scheduleDraw }),
    scheduleDraw,
    // Renderer introspection for the pane manager's diagnostics; this wiring is
    // rebuilt onto CPU after a context loss, so it reflects the live draw path.
    rendererKind: () => pending.kind,
    adapterInfo: () => pending.adapterInfo,
    // Worker path: let the facade re-drain OSC/bell the instant the worker pushes them
    // (not a chunk late). In-process leaves strategy.onSideChannel unset → no-op (its
    // post-process() drain is already synchronous + current).
    onEngineSideChannel: (handler: () => void) => strategy.onSideChannel?.(handler),
    // Gate BOTH the main-thread scheduler (in-process draws + overlay) and, on the
    // worker path, the worker's autonomous render loop — it draws on its own rAF, so
    // suspension must be posted across the seam (no-op for in-process strategies).
    setDrawSuspended: (suspended: boolean) => {
      strategy.setDrawSuspended?.(suspended)
      drawScheduler.setSuspended(suspended)
    },
    ...replySurface,
    dispose: teardown
  }

  return { controller, strategy, teardown }
}
