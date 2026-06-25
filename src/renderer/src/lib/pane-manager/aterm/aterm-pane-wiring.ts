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
import { createAtermTitleChannel } from './aterm-title-channel'
import { createAtermProcessPump } from './aterm-process-pump'
import { attachAtermGridReflow, type AtermMetrics } from './aterm-grid-reflow'
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
import { ATERM_RENDERER_FONT_PX } from './aterm-pane-controller-types'

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
  const initialDpr = window.devicePixelRatio || 1
  // `pending` was rasterized at the dpr captured when the strategy STARTED loading;
  // the async load (GPU init can take seconds) gives the window time to settle to a
  // different dpr (e.g. a headless window born at 2 settling to 1), which would leave
  // cell metrics frozen at the load-time dpr → wrong column count. Re-rasterize to the
  // live dpr now (set_px is a no-op when unchanged) so metrics + dpr agree from frame 1.
  term.set_px(Math.round(ATERM_RENDERER_FONT_PX * initialDpr))
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

  let draw: () => void = () => undefined
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
    scheduleDraw
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

  // draw = present the engine grid, then (GPU) paint search on the overlay. The
  // CPU strategy's drawFrame already overlays search on its own 2d canvas. The
  // a11y mirror is scheduled (debounced) here so it tracks rendered content.
  draw = (): void => {
    strategy.drawFrame()
    searchOverlay?.paint(searchMatches, searchActiveIndex)
    a11yMirror.schedule()
  }

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
    getGrid,
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

  // Focus the helper textarea on canvas click.
  const onPointerDown = (): void => textarea.focus()
  canvas.addEventListener('pointerdown', onPointerDown)

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
    canvas.removeEventListener('pointerdown', onPointerDown)
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
    // Buffer/grid reads + scroll/selection commands (live engine state); extracted
    // to keep this file focused.
    ...buildAtermEngineReads(term, scheduleDraw, () => disposed),
    // CSS cell size = live device cell px / current dpr. `metrics` is updated in
    // place by the grid reflow on a DPI change, so this tracks the real cell size
    // (xterm's `_renderService.dimensions.css.cell`) without a pane rebuild.
    cellSizeCss: () => ({
      width: term.cell_width / metrics.dpr,
      height: term.cell_height / metrics.dpr
    }),
    linkAt: (row: number, col: number) => {
      const hit = term.link_at(row, col)
      return hit ? { url: hit.url, kind: hit.kind } : null
    },
    ...searchApi,
    setFileLinkOpener: (fn: AtermFileLinkOpener) => void (shared.fileLinkOpener = fn),
    setUrlLinkContext: (context: AtermLinkContext) => void (shared.activeLinkContext = context),
    lastMouseReport: () => eventReportingInput.lastMouseReport(),
    // aterm-native serialize (replaces xterm's SerializeAddon). The wasm methods are
    // snake_case; undefined scrollback → all history.
    serialize: (scrollbackRows?: number) => term.serialize(scrollbackRows),
    serializeScrollback: (maxRows?: number) => term.serialize_scrollback(maxRows),
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
    setDrawSuspended: (suspended: boolean) => drawScheduler.setSuspended(suspended),
    ...replySurface,
    dispose: teardown
  }

  return { controller, strategy, teardown }
}
