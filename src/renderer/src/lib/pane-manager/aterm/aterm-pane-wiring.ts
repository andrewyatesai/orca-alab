import { attachAtermTextareaInput, encodeAtermKeyForHost } from './aterm-textarea-input'
import { attachAtermCursorBlink } from './aterm-cursor-blink'
import { buildAtermThemeMutators } from './aterm-controller-theme-mutators'
import { attachAtermPointerInputs } from './aterm-pointer-input-bundle'
import { createAtermPaneGridSizing, type AtermPaneGridSizing } from './aterm-pane-grid-sizing'
import type { AtermFileLinkOpener, AtermLinkProviderSource } from './aterm-link-input'
import { createAtermDrawScheduler } from './aterm-draw-scheduler'
import { createAtermEffectsDrive, type AtermEffectsDriveEngine } from './aterm-effects-drive'
import { createAtermPaneSearchState } from './aterm-pane-search-state'
import type { AtermLinkContext } from './aterm-url-link-routing'
import { buildAtermRendererReplySurface } from './aterm-renderer-reply-surface'
import { mountAtermPaneCanvasAdjuncts } from './aterm-pane-canvas-adjuncts'
import { buildAtermEngineReads } from './aterm-engine-reads'
import { wireWorkerStrategyHooks } from './aterm-worker-strategy-hookup'
import { buildAtermSerializeMembers } from './aterm-serialize-members'
import { createAtermTitleChannel } from './aterm-title-channel'
import { createAtermProcessPump } from './aterm-process-pump'
import type { AtermMetrics } from './aterm-grid-reflow'
import { createAtermPanePresenter } from './aterm-pane-present'
import { applyTerminalPrimaryFontThenReflow } from './inject-terminal-primary-font'
import { attachAtermCanvasFocus } from './aterm-canvas-focus'
import { applyAtermEngineSettings } from './aterm-engine-settings-apply'
import { wireAtermWindowChrome } from './aterm-effects-settings'
import { wireAtermPaneSpill } from './aterm-pane-spill-wiring'
import type { AtermPaneWiringConfig, AtermWiredPane } from './aterm-pane-wiring-types'
import type { AtermPaneController } from './aterm-pane-controller-types'
import { createAtermControllerOptionReaders } from './aterm-controller-option-readers'
import { driveAtermRainPulse } from './aterm-rain-pulse'

// The wiring seam types (config in, wired pane out, context-loss-surviving late
// bindings) live in aterm-pane-wiring-types; re-exported so importers keep one home.
export type {
  AtermPaneWiringConfig,
  AtermSharedLateBindings,
  AtermWiredPane
} from './aterm-pane-wiring-types'

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
  const { getFontPx, getLineHeight, getFontFamily, getFontWeight } = readers
  // Mutable metrics shared with the input-handler deps: a later host DPI change
  // re-rasterizes the engine (term.set_px) and updates these in place via the grid
  // reflow, so the grid + overlays resize instead of freezing at construction dpr.
  const metrics: AtermMetrics = { dpr: window.devicePixelRatio || 1, cellWidth: 0, cellHeight: 0 }
  // `pending` was rasterized at the dpr captured when the strategy STARTED loading;
  // the async load (GPU init can take seconds) gives the window time to settle to a
  // different dpr (e.g. a headless window born at 2 settling to 1), which would leave
  // cell metrics frozen at the load-time dpr → wrong column count. Re-rasterize to the
  // live dpr now (set_px is a no-op when unchanged) so metrics + dpr agree from frame 1.
  term.set_px(Math.round(getFontPx() * metrics.dpr))
  // Apply the user's line-height before reading cell metrics so the grid is sized to
  // the real (scaled) cell height from frame 1; set_px re-applies it on later changes.
  term.set_line_height(getLineHeight())
  metrics.cellWidth = term.cell_width
  metrics.cellHeight = term.cell_height
  let disposed = false

  // Grid state + reflow + the explicit resize override live in the sizing module;
  // assigned right after the strategy binds (closures below read it at runtime).
  let gridSizing!: AtermPaneGridSizing
  // Facade subscribers notified after each mouse-driven selection mutation, so
  // onSelectionChange fires without waiting for PTY output.
  const selectionMutationListeners = new Set<() => void>()

  // draw (rAF) + presentNow (interactive fast path) are assigned from the presenter
  // below, once the strategy + grid reflow exist. The drawScheduler + process pump
  // capture them by closure and only invoke them at runtime (after wiring completes).
  let draw: () => void = () => undefined
  let presentNow: () => void = () => undefined
  // Late-bound like draw/presentNow: engineSettings (created below) owns the live
  // OSC 12 → glow-colour follow; the pump + worker side-channel hooks fire it.
  let syncCursorColorEffects: () => void = () => undefined
  const drawScheduler = createAtermDrawScheduler(() => draw())
  const scheduleDraw = (): void => {
    if (!disposed) {
      drawScheduler.schedule()
    }
  }

  // Cross-pane spill (stage 3, in-process only): flips stage 2's fail-closed
  // capability seam live + builds the presenter's rev-gated per-paint blit.
  const spill = wireAtermPaneSpill(term, pending.memory, shared, scheduleDraw, () => disposed)

  // Search state (matches/active-index/re-index flag) + controller + API live in
  // their own module; getRows closes over the late-assigned gridSizing.
  const searchState = createAtermPaneSearchState({
    term,
    metrics,
    isDisposed: () => disposed,
    getRows: () => gridSizing.grid().rows,
    scheduleDraw
  })

  const titleChannel = createAtermTitleChannel(term)

  const process = createAtermProcessPump({
    term,
    inputSink,
    isDisposed: () => disposed,
    emitTitleIfChanged: titleChannel.emitIfChanged,
    hasActiveSearchQuery: () => searchState.searchController.hasActiveQuery(),
    markSearchRefresh: searchState.markSearchRefresh,
    syncCursorColor: () => syncCursorColorEffects(),
    // Present a keystroke echo immediately (coalesced to once per frame) instead of
    // waiting a full rAF — see presentNow. Bulk output still coalesces onto rAF.
    scheduleDraw: () => presentNow()
  })

  const { selectionInput, scrollInput, eventReportingInput, linkInput, linkTooltip, syncDpr } =
    attachAtermPointerInputs({
      canvas,
      textarea,
      term,
      metrics,
      inputSink,
      controllerOptions,
      shared,
      getRows: () => gridSizing.grid().rows,
      scheduleDraw,
      isDisposed: () => disposed,
      onSelectionChanged: () => selectionMutationListeners.forEach((listener) => listener())
    })

  // Bind the strategy's painter now that search + getters exist (they depend on
  // the engine the strategy created). The GPU strategy forwards context loss to
  // the controller (config.onContextLoss) so it can swap to CPU; CPU ignores it.
  const strategy = pending.bindPainter({
    drawScheduler,
    searchController: searchState.searchController,
    isDisposed: () => disposed,
    getDpr: () => metrics.dpr,
    getRows: () => gridSizing.grid().rows,
    getSearchMatches: searchState.getSearchMatches,
    getSearchActiveIndex: searchState.getSearchActiveIndex,
    takeSearchRefresh: searchState.takeSearchRefresh,
    getHoveredLinkSpan: () => linkInput.hoveredSpan(),
    getFgColor: () => themeColors.fg,
    onContextLoss: (seedAnsi?: string) => config.onContextLoss(seedAnsi)
  })

  // The DOM stacked around the grid canvas: the (GPU-only) search-highlight
  // overlay, the overlay scrollbar, and the off-screen ARIA output mirror.
  const { searchOverlay, scrollbarOverlay, a11yMirror } = mountAtermPaneCanvasAdjuncts({
    canvas,
    liveRegion,
    term,
    metrics,
    needsSearchOverlay: strategy.needsSearchOverlay === true,
    getRows: () => gridSizing.grid().rows,
    getCols: () => gridSizing.grid().cols,
    getHoveredLinkSpan: () => linkInput.hoveredSpan(),
    getFgColor: () => themeColors.fg,
    scheduleDraw,
    isDisposed: () => disposed
  })

  // draw + presentNow are wired from the presenter just after the grid reflow exists
  // (it's one of the presenter's deps). See below.

  // Size the real grid + attach the container/DPI reflow; explicit resizes
  // (snapshot replay, mobile-fit) pin an override the reflow honors.
  gridSizing = createAtermPaneGridSizing({
    term,
    container,
    metrics,
    strategy,
    getFontPx,
    getLineHeight,
    resizeSink,
    // Refresh the pointer/scroll/link handlers' cached metrics + re-derive the
    // window chrome (this also marks the wired engine chrome-capable — every
    // real drawer offsets the canvas box): every cell-metrics re-rasterization
    // funnels through this seam (reapplyMetrics + forceReflow, on both paths).
    syncDependents: wireAtermWindowChrome(term, syncDpr),
    scheduleDraw,
    isDisposed: () => disposed
  })

  // Worker path: forward engine query replies to the PTY + re-reflow on worker
  // re-rasterization. No-op for the in-process CPU/GPU strategies.
  wireWorkerStrategyHooks({
    strategy,
    term,
    metrics,
    inputSink,
    forceReflow: () => gridSizing.reflow.forceReflow(),
    emitTitleIfChanged: titleChannel.emitIfChanged,
    syncCursorColor: () => syncCursorColorEffects(),
    isDisposed: () => disposed
  })

  // Effects animation drive: rAF cadence only while the engine reports an active
  // animation, zero scheduled work once settled. The worker-backed term lacks the
  // effects methods (its worker ticks them itself), so the drive no-ops there.
  const effectsDrive = createAtermEffectsDrive({
    term: term as AtermEffectsDriveEngine,
    scheduleDraw,
    isDisposed: () => disposed
  })

  // Wire the paint path now that the strategy + grid reflow exist: the rAF draw and
  // the interactive presentNow fast path share one presenter.
  const presenter = createAtermPanePresenter({
    strategy,
    searchOverlay,
    a11yMirror,
    gridReflow: gridSizing.reflow,
    drawScheduler,
    scheduleDraw,
    isDisposed: () => disposed,
    getSearchMatches: searchState.getSearchMatches,
    getSearchActiveIndex: searchState.getSearchActiveIndex,
    effectsDrive,
    spillBlit: spill.spillBlit
  })
  draw = presenter.draw
  presentNow = presenter.presentNow

  // Honor terminalFontFamily + terminalFontWeight: swap in the host-resolved
  // weight-closest face (+ the family's real bold face for SGR bold, when it ships
  // one) and reflow once the bytes load; a bundled/unresolvable family is a no-op.
  // A live family/weight change applies on the next opened terminal.
  applyTerminalPrimaryFontThenReflow(
    term,
    getFontFamily(),
    getFontWeight(),
    () => disposed,
    () => gridSizing.reflow.forceReflow()
  )

  const textareaInput = attachAtermTextareaInput({
    textarea,
    term,
    canvas,
    metrics,
    themeColors,
    getRows: () => gridSizing.grid().rows,
    redraw: scheduleDraw,
    inputSink,
    pasteSink,
    copySelection: () => selectionInput.copySelection(),
    getMacOptionIsMeta: controllerOptions?.getMacOptionIsMeta,
    getCustomKeyEventHandler: controllerOptions?.getCustomKeyEventHandler,
    getImeAnchor: controllerOptions?.getImeAnchor
  })

  // Blink the cursor (focused) + draw it hollow (unfocused); the engine paints the
  // cursor but has no timer/focus model of its own.
  const cursorBlink = attachAtermCursorBlink({
    term,
    textarea,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    getCursorBlink: controllerOptions?.getCursorBlink,
    isDrawSuspended: drawScheduler.isSuspended
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
    isDisposed: () => disposed,
    scheduleDraw,
    refreshCursorBlink: cursorBlink.refresh
  })
  syncCursorColorEffects = engineSettings.syncCursorColor

  // Report the initial grid to the PTY only when it came from real layout. An
  // unmeasured container (pre-layout remount, hidden pane) yields the 80x24
  // fallback — pushing that placeholder onto a live reattached PTY kernel-
  // SIGWINCHes TUIs into a placeholder relayout and back (resetting alt-screen
  // viewports to the top); the reflow observer reports the real grid as soon
  // as layout lands.
  if (gridSizing.initialGridMeasured) {
    resizeSink(gridSizing.grid().cols, gridSizing.grid().rows)
  }
  scheduleDraw()

  const replySurface = buildAtermRendererReplySurface({
    term,
    metrics,
    themeColors,
    getGrid: gridSizing.grid,
    scheduleDraw
  })

  const teardown = (): void => {
    if (disposed) {
      return
    }
    disposed = true
    drawScheduler.dispose()
    effectsDrive.dispose()
    a11yMirror.dispose()
    gridSizing.reflow.dispose()
    textareaInput.dispose()
    cursorBlink.dispose()
    canvasFocus.dispose()
    engineSettings.dispose()
    selectionInput.dispose()
    scrollInput.dispose()
    eventReportingInput.dispose()
    linkInput.dispose()
    linkTooltip.dispose()
    scrollbarOverlay.dispose()
    searchOverlay?.dispose()
    spill.unregister()
    strategy.dispose()
  }
  const rainPulseDraw = strategy.setDrawSuspended ? undefined : scheduleDraw

  const controller: AtermPaneController = {
    process,
    // Worker commands schedule internally; only in-process engines receive a host draw.
    noteMatrixRainPulse: (pulse) => void driveAtermRainPulse(term, pulse, rainPulseDraw),
    displayOffset: () => term.display_offset,
    // Buffer/grid reads (incl. cellSizeCss + linkAt) + scroll/selection commands (live
    // engine state); extracted to keep this file focused.
    ...buildAtermEngineReads(term, metrics, scheduleDraw, () => disposed),
    ...searchState.searchApi,
    setFileLinkOpener: (fn: AtermFileLinkOpener) => void (shared.fileLinkOpener = fn),
    setUrlLinkContext: (context: AtermLinkContext) => void (shared.activeLinkContext = context),
    setLinkProviderSource: (src: AtermLinkProviderSource) => void (shared.linkProviderSource = src),
    bindSpillPaneKey: spill.bindSpillPaneKey,
    onSelectionMutation: (handler: () => void) => void selectionMutationListeners.add(handler),
    resize: (nextCols: number, nextRows: number) => gridSizing.resize(nextCols, nextRows),
    fitToContainer: () => gridSizing.fitToContainer(),
    keyboardModeBits: () => term.keyboard_mode_bits,
    // Same engine encoder the textarea keydown path selects (live mode
    // in-process, snapshot mode bits on the worker path).
    encodeKeyForHost: (key: string, mods: number) => encodeAtermKeyForHost(term, key, mods),
    lastMouseReport: () => eventReportingInput.lastMouseReport(),
    // aterm-native serialize (replaces xterm's SerializeAddon): sync (engine / worker
    // cached blob) + awaitable (worker round-trip for fresh history). undefined → all.
    ...buildAtermSerializeMembers(term, strategy),
    title: titleChannel.title,
    onTitleChange: titleChannel.onTitleChange,
    gridSize: () => gridSizing.grid(),
    // Toggle the engine's fail-closed OSC 52 write gate so it queues OSC 52 set
    // events for the facade to drain; the host still enforces the user setting.
    setClipboardWriteAuthorized: (allowed: boolean) =>
      allowed ? term.authorize_clipboard_write() : term.revoke_clipboard_write(),
    // Engine-side fail-closed OSC 9/99/777 gate, synced from the user's notification
    // settings by the lifecycle layer (mirrors the OSC 52 clipboard gate above).
    setNotificationsAuthorized: (allowed: boolean) => term.authorize_notifications(allowed),
    element,
    textarea,
    // Live re-theme + selection-focus mutators (re-style the engine in place, no
    // pane rebuild). `metrics` is passed by reference so re-theme reads the
    // current cell size after a DPI change, not the construction one.
    ...buildAtermThemeMutators({ term, themeColors, metrics, scheduleDraw }),
    // Live-apply ligatures / scrollback / default cursor style / cursor blink to this
    // OPEN pane on a settings change (re-reads the live readers), matching how
    // theme/size apply live.
    reapplyEngineSettings: engineSettings.reapply,
    scheduleDraw,
    // Renderer introspection for the pane manager's diagnostics; this wiring is
    // rebuilt onto CPU after a context loss, so it reflects the live draw path.
    rendererKind: () => pending.kind,
    adapterInfo: () => pending.adapterInfo,
    // Worker path: let the facade re-drain OSC/bell the instant the worker pushes them
    // (not a chunk late). In-process leaves strategy.onSideChannel unset → no-op (its
    // post-process() drain is already synchronous + current).
    onEngineSideChannel: (handler: () => void) => strategy.onSideChannel?.(handler),
    // Parse fence for the replay guard: worker parsing is async, so the guard stays
    // open until the fence resolves; in-process parse is synchronous → immediate.
    settle: () => strategy.settle?.() ?? Promise.resolve(),
    // Gate BOTH the main-thread scheduler (in-process draws + overlay) and, on the
    // worker path, the worker's autonomous render loop — it draws on its own rAF, so
    // suspension must be posted across the seam (no-op for in-process strategies).
    setDrawSuspended: (suspended: boolean) => {
      strategy.setDrawSuspended?.(suspended)
      drawScheduler.setSuspended(suspended)
      cursorBlink.refreshEffectsVisibility()
    },
    ...replySurface,
    dispose: teardown
  }

  return { controller, strategy, teardown }
}
