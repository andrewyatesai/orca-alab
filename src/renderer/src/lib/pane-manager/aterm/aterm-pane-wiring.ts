import { attachAtermTextareaInput } from './aterm-textarea-input'
import { attachAtermScrollInput } from './aterm-scroll-input'
import { attachAtermSelectionInput } from './aterm-selection-input'
import { attachAtermCursorBlink } from './aterm-cursor-blink'
import { drainAtermReplies } from './aterm-reply-drain'
import { applyAtermLiveTheme } from './aterm-theme-colors'
import { attachAtermEventReportingInput } from './aterm-event-reporting-input'
import { computeGrid } from './aterm-grid-size'
import { attachAtermLinkInput, type AtermFileLinkOpener } from './aterm-link-input'
import { createAtermSearchController, type AtermSearchMatch } from './aterm-search'
import { createAtermDrawScheduler } from './aterm-draw-scheduler'
import { attachAtermDprTracker } from './aterm-dpr-tracker'
import { buildAtermSearchApi } from './aterm-search-api'
import { createAtermUrlOpener, type AtermLinkContext } from './aterm-url-link-routing'
import { buildAtermRendererReplySurface } from './aterm-renderer-reply-surface'
import { copyAtermSelectionToClipboard } from './aterm-clipboard-copy'
import { createAtermSearchOverlayCanvas } from './aterm-search-overlay-canvas'
import { createAtermA11yMirror } from './aterm-a11y-mirror'
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

/** Everything the wiring needs to turn a loaded strategy into a live pane. */
export type AtermPaneWiringConfig = {
  pending: AtermPendingStrategy
  canvas: HTMLCanvasElement
  container: HTMLElement
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
  const { pending, canvas, container, textarea, liveRegion, themeColors, shared } = config
  const { inputSink, resizeSink, pasteSink, controllerOptions } = config
  const term = pending.term
  const cellWidth = pending.cellWidth
  const cellHeight = pending.cellHeight

  let dpr = window.devicePixelRatio || 1
  let disposed = false
  let searchRefreshPending = false

  const initialGrid = computeGrid(container, dpr, cellWidth, cellHeight)
  let cols = initialGrid.cols
  let rows = initialGrid.rows

  let searchMatches: AtermSearchMatch[] = []
  let searchActiveIndex = -1

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
    // Follow the bottom on new output ONLY if already at the bottom (aterm SCR-1).
    const wasAtBottom = term.display_offset === 0
    term.process(new TextEncoder().encode(data))
    // aterm is the authoritative query responder — drain + forward its replies.
    drainAtermReplies(term, inputSink)
    if (wasAtBottom && term.display_offset !== 0) {
      term.scroll_to_bottom()
    }
    searchRefreshPending ||= searchController.hasActiveQuery()
    scheduleDraw()
  }

  const selectionDeps = {
    canvas,
    term,
    dpr,
    cellWidth,
    cellHeight,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    onCopy: copyAtermSelectionToClipboard,
    getCopyOnSelect: controllerOptions?.getCopyOnSelect
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

  const eventReportingInput = attachAtermEventReportingInput({
    canvas,
    textarea,
    term,
    dpr,
    cellWidth,
    cellHeight,
    inputSink,
    isDisposed: () => disposed
  })

  // URL/file-path openers are held in `shared` so a GPU→CPU rebuild keeps the
  // late-bound openers the lifecycle set on the prior controller.
  const openUrl = createAtermUrlOpener(() => shared.activeLinkContext)

  const linkDeps = {
    canvas,
    term,
    dpr,
    cellWidth,
    cellHeight,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    openUrl,
    getFileLinkOpener: () => shared.fileLinkOpener
  }
  const linkInput = attachAtermLinkInput(linkDeps)

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
    getDpr: () => dpr,
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
        cellWidth,
        cellHeight,
        getDpr: () => dpr,
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
    cellWidth,
    cellHeight,
    isDisposed: () => disposed,
    getRows: () => rows,
    getSearchMatches: () => searchMatches,
    getSearchActiveIndex: () => searchActiveIndex
  })

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
    strategy.resize(rows, cols)
    resizeSink(cols, rows)
    scheduleDraw()
  }

  const resizeObserver = new ResizeObserver(reflowGrid)
  resizeObserver.observe(container)
  // Size the real grid + report it so the PTY matches the canvas.
  strategy.resize(rows, cols)

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

  const getGrid = (): { cols: number; rows: number } => ({ cols, rows })
  const replySurface = buildAtermRendererReplySurface({
    term,
    cellWidth,
    cellHeight,
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
    resizeObserver.disconnect()
    dprTracker.dispose()
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
    setFileLinkOpener: (fn: AtermFileLinkOpener) => void (shared.fileLinkOpener = fn),
    setUrlLinkContext: (context: AtermLinkContext) => void (shared.activeLinkContext = context),
    lastMouseReport: () => eventReportingInput.lastMouseReport(),
    // Re-theme this live engine in place (host theme change), avoiding a pane
    // rebuild that would drop scrollback. Caller (applyTerminalAppearance) only
    // iterates live panes; scheduleDraw no-ops if disposed.
    updateTheme: (colors: AtermThemeColors) => {
      applyAtermLiveTheme(term, colors, cellWidth, cellHeight)
      // Mutate the shared themeColors IN PLACE (not reassign) so the live getters
      // — link-underline fg + the reply surface's OSC 10/11 color source, both of
      // which captured this object — read the new theme without a pane rebuild.
      Object.assign(themeColors, colors)
      scheduleDraw()
    },
    ...replySurface,
    dispose: teardown
  }

  return { controller, strategy, teardown }
}
