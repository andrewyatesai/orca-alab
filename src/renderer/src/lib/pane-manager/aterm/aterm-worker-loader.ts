import { decideAtermGpu } from './aterm-gpu-auto-policy'
import { MIN_GRID_COLS, MIN_GRID_ROWS } from './aterm-grid-size'
import { createWorkerBackedTerm, type WorkerBackedTerm } from './aterm-worker-term'
import { createAtermWorkerOverlay, type AtermWorkerOverlay } from './aterm-worker-overlay'
import { acquireAtermSharedWorkerPane } from './aterm-shared-render-worker'
import type { AtermHoveredLinkSpan } from './aterm-link-underline-overlay'
import { e2eConfig } from '@/lib/e2e-config'
import type { AtermPendingStrategy } from './aterm-strategy-select'
import type { AtermDrawerBuildConfig, AtermPainterBinding } from './aterm-drawer-config'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermWorkerState } from './aterm-render-worker-protocol'

// Per-pane client of the SHARED render worker (aterm-shared-render-worker): acquires
// a paneId slot, transfers the pane's OffscreenCanvas, awaits the pane's first STATE,
// and builds the worker-backed `term` the controller binds to (synchronous snapshot
// reads + posted mutations). Fonts never pass through here — the manager ships them
// to the worker once; the pane engine seeds from the worker-resident copy.

// Cap the wait for the worker's 'booted' ack (posted on receiving the manager's
// fonts message) so a wedged/dead worker — failed script load, spawn failure, a
// crash before its message loop — can't hang pane creation. A healthy worker acks
// near-instantly; panes joining an already-booted worker skip this phase entirely.
const WORKER_BOOT_TIMEOUT_MS = 4000
// After the ack the worker is provably alive and building this pane's engine: wasm
// compile + font parse + (possibly software) GL acquire take seconds, and CONCURRENT
// pane opens (app start, session restore, several panes at once) contend and stretch
// that past any frame-scale deadline. Killing a live build silently downgrades the
// pane to the in-process path, so wait it out under a longer — still bounded, a hung
// GL acquire must not blank the pane forever — total cap measured from the init post.
// PER PANE: a slow pane failing this deadline releases only ITS slot; other panes on
// the shared worker are untouched.
const WORKER_FIRST_FRAME_TIMEOUT_MS = 15_000

export async function loadAtermWorkerEngine(
  config: AtermDrawerBuildConfig
): Promise<AtermPendingStrategy> {
  const { canvas, themeColors, fontPx, lineHeight } = config

  // Fallible (font/asset fetch) BEFORE the canvas transfer, so a failure here throws
  // with the canvas still intact and loadAtermStrategy can fall back in-process.
  const pane = await acquireAtermSharedWorkerPane()
  const post = pane.post

  // The WORKER engine takes the GPU path when the policy allows (GPU render + present
  // off-main too); CPU is the guaranteed fallback (a 'fallback' message rebuilds CPU
  // on the same canvas if WebGL can't be acquired in the worker).
  const useGpuWorker = decideAtermGpu().useGpu
  let fellBackToCpuWorker = false

  let backed: WorkerBackedTerm | null = null
  // Main-thread stacked overlay for search highlights + link underline (the worker owns
  // the pane canvas, so these 2d marks paint on a sibling canvas from the snapshot).
  let overlay: AtermWorkerOverlay | null = null
  // Hidden-pane gating: mirror the worker's suspended flag so the main thread also stops
  // repainting the overlay each STATE while suspended (the worker keeps posting STATE so
  // sync snapshot reads + a11y stay fresh; only the visible paint is gated).
  let workerSuspended = false
  // The applied (reconciled) dpr the worker rendered at — set from the wiring's getDpr in
  // bindPainter so the overlay's CSS box tracks the pane canvas through a DPI settle /
  // fractional dpr (live devicePixelRatio can diverge from the rendered dpr). Defaults to
  // live dpr until bindPainter runs (the overlay only paints after the first frame anyway).
  let overlayGetDpr: () => number = () => window.devicePixelRatio || 1
  // The main-thread hovered link span (provider links are detected on the main
  // thread — the worker's own link_at never sees them), set from bindPainter.
  let overlayGetSpan: () => AtermHoveredLinkSpan | null = () => null
  let firstResolved = false
  let resolveFirst: (state: AtermWorkerState) => void = () => undefined
  let rejectFirst: (err: Error) => void = () => undefined
  const firstState = new Promise<AtermWorkerState>((resolve, reject) => {
    resolveFirst = resolve
    rejectFirst = reject
  })

  // The hover cursor last written to the pane canvas — skip the CSSOM write on the
  // common steady-state frames where it's unchanged (most frames during streaming).
  let lastHoverCursor = ''

  // Shared-worker crash (a wasm RuntimeError poisons the module for EVERY engine in
  // it, so the manager retires the worker and fires each pane's onCrash): without
  // recovery this pane silently freezes at its last frame while keystrokes keep
  // flowing. Route into the controller's context-loss rebuild — the same path that
  // already rebuilds on a FRESH canvas (the transferred one is poisoned) with an
  // in-process CPU engine and replays mid-swap output. Seeded with THIS pane's last
  // serialized cache so the grid repaints instead of waiting blank for the next PTY
  // byte. ONE attempt: the rebuilt path has no worker left to crash.
  let onRuntimeCrash: ((seedAnsi?: string) => void) | null = null
  let workerCrashed = false
  const recoverFromWorkerCrash = (message: string): void => {
    if (workerCrashed) {
      return
    }
    workerCrashed = true
    console.error(
      '[aterm] shared render worker crashed; rebuilding the pane on the in-process CPU path:',
      message
    )
    // The debounced serialize cache is the pre-crash state (aterm's own replayable
    // ANSI) — the strongest resync available without the dead engine.
    const seedAnsi = backed?.term.serialize() || undefined
    if (onRuntimeCrash) {
      onRuntimeCrash(seedAnsi)
      return
    }
    // Crash before the painter bound (no rebuild seam yet): fail the first-frame wait
    // NOW (don't sit out the build deadline on a dead worker) so loadAtermStrategy's
    // fallback rebuilds in-process on a fresh canvas. The manager already terminated
    // the worker and dropped every pane slot.
    rejectFirst(new Error(`aterm worker crashed before its first frame: ${message}`))
  }
  pane.onCrash(recoverFromWorkerCrash)

  pane.onEvent((data) => {
    switch (data.type) {
      case 'state':
        if (!firstResolved) {
          firstResolved = true
          resolveFirst(data)
        } else {
          // Keep state fresh for sync reads + a11y even while hidden, but skip the
          // visible overlay repaint when suspended (resume re-paints the latest state).
          backed?.applyState(data)
          // Drive the pane canvas hover cursor from the worker's computed hoverCursor
          // ('pointer' over a link, else ''): the worker owns link detection, so this is
          // the single source of truth — the sync link_at snapshot lags a frame and never
          // updates the cursor on a stationary hover. Only write when it changed.
          if (data.hoverCursor !== lastHoverCursor) {
            canvas.style.cursor = data.hoverCursor
            lastHoverCursor = data.hoverCursor
          }
          if (!workerSuspended) {
            overlay?.paint(data)
          }
        }
        if (e2eConfig.exposeStore) {
          window.__atermWorkerRenderState = data
        }
        return
      case 'reply':
        backed?.pushReply(data.data)
        return
      case 'osc':
        backed?.pushOsc(data.events)
        return
      case 'bell':
        backed?.pushBell()
        return
      case 'queryResult':
        backed?.resolveQuery(data.id, data.value)
        return
      case 'serializedCache':
        backed?.applySerializedCache(data.full, data.scrollback)
        return
      case 'error':
        // GPU acquire failed in the worker → rebuild THIS pane as CPU on the same
        // canvas (it can't be re-transferred) so it renders off-main instead of blank.
        if (!fellBackToCpuWorker) {
          fellBackToCpuWorker = true
          post({ type: 'fallback' })
        }
    }
  })

  // Hand the canvas to the worker; from here ONLY the worker may draw to it.
  const offscreen = canvas.transferControlToOffscreen()
  post(
    {
      type: 'init',
      engine: useGpuWorker ? 'gpu' : 'cpu',
      canvas: offscreen,
      // The wiring re-applies the user's fontPx/line-height via the term's posted
      // set_px/set_line_height/resize; start at the MIN grid. The user's line-height is
      // threaded here so the FIRST snapshot's cell box is already correct (no over-counted
      // initial rows / spurious first-open SIGWINCH when terminalLineHeight != 1).
      rows: MIN_GRID_ROWS,
      cols: MIN_GRID_COLS,
      fontPx,
      lineHeight: lineHeight ?? 1,
      themeColors
    },
    [offscreen]
  )

  // Wait for this pane's first frame so the controller's construction-time reads
  // (cell_width/height) are real before wireAtermPane runs. Race a two-phase deadline
  // so a wedged worker (no ack, no state, no error) can't hang pane creation forever,
  // while a live one gets to finish its seconds-long engine build (concurrent pane
  // opens contend; killing a healthy build here would silently drop the pane to the
  // in-process path). Boot timeout retires the SHARED worker (every pane on it shares
  // the wedge); a build timeout frees only THIS pane's slot.
  let initial: AtermWorkerState
  try {
    initial = await Promise.race([
      firstState,
      new Promise<never>((_, reject) => {
        setTimeout(() => {
          if (!pane.isBooted()) {
            pane.reportBootWedged(`aterm worker boot timed out (${WORKER_BOOT_TIMEOUT_MS}ms)`)
            reject(new Error(`aterm worker boot timed out (${WORKER_BOOT_TIMEOUT_MS}ms)`))
            return
          }
          setTimeout(
            () =>
              reject(
                new Error(`aterm worker first frame timed out (${WORKER_FIRST_FRAME_TIMEOUT_MS}ms)`)
              ),
            WORKER_FIRST_FRAME_TIMEOUT_MS - WORKER_BOOT_TIMEOUT_MS
          )
        }, WORKER_BOOT_TIMEOUT_MS)
      })
    ])
  } catch (err) {
    // Free only THIS pane's slot: a mid-build engine is freed by the worker's
    // disposed flag, and the manager terminates the worker when no pane remains.
    // Both are safe no-ops when the generation was already retired (crash/wedge).
    post({ type: 'dispose' })
    pane.release()
    throw err
  }
  backed = createWorkerBackedTerm({ post, initial })
  // Route the engine setters applyAtermEngineSettings calls (minimum contrast / word
  // separators / bg+cursor opacity / kitty policy) as worker commands, mirroring
  // set_ligatures/set_scrollback_limit. Attached here, surgically, until the planned
  // worker-term refactor absorbs them.
  backed.term.set_minimum_contrast = (ratio: number): void =>
    post({ type: 'setMinimumContrast', ratio })
  backed.term.set_word_separators = (separators?: string | null): void =>
    post({ type: 'setWordSeparators', separators: separators ?? null })
  backed.term.set_background_opacity = (opacity: number): void =>
    post({ type: 'setBackgroundOpacity', opacity })
  backed.term.set_cursor_opacity = (opacity: number): void =>
    post({ type: 'setCursorOpacity', opacity })
  backed.term.set_kitty_keyboard_enabled = (enabled: boolean): void =>
    post({ type: 'setKittyKeyboardEnabled', enabled })
  // The worker owns the pane canvas, so search highlights + the link underline paint on
  // a main-thread stacked overlay driven by the snapshot (works for CPU + GPU worker).
  overlay = createAtermWorkerOverlay(
    canvas,
    () => themeColors.fg,
    () => overlayGetDpr(),
    () => overlayGetSpan()
  )
  overlay.paint(initial)

  const bindPainter = (binding: AtermPainterBinding): AtermDrawStrategy => {
    // Feed the overlay the wiring's reconciled dpr so its CSS box tracks the pane canvas
    // through a DPI settle (live devicePixelRatio can diverge from the rendered dpr).
    overlayGetDpr = binding.getDpr
    // Provider-link hover spans paint here (the worker underlines only its own
    // engine-detected links from state.hoverLink).
    overlayGetSpan = binding.getHoveredLinkSpan
    // A runtime crash rebuilds through the controller's context-loss seam (fresh
    // canvas + in-process CPU). Deferred when the crash beat this bind: the
    // rebuild seam only becomes callable once the wiring returns.
    onRuntimeCrash = binding.onContextLoss
    if (workerCrashed) {
      setTimeout(() => binding.onContextLoss(backed?.term.serialize() || undefined), 0)
    }
    return {
      term: backed!.term,
      getCanvas: () => canvas,
      // The worker presents the engine grid (incl. selection, engine-drawn); search +
      // link overlays paint on the main-thread stacked overlay above (snapshot-driven),
      // so the controller's in-process search-overlay path stays off.
      needsSearchOverlay: false,
      drawFrame: () => post({ type: 'draw' }),
      resize: (rows, cols) => backed!.term.resize(rows, cols),
      // Hidden-pane gating across the seam: the worker renders on its own rAF, so pause
      // its draw loop (+ the main-thread overlay repaint) when the pane is hidden. The
      // worker schedules one draw on resume so the pane shows its latest state.
      setDrawSuspended: (next) => {
        workerSuspended = next
        post({ type: 'setDrawSuspended', suspended: next })
      },
      onReply: (handler) => backed!.onReply(handler),
      onMetricsChange: (handler) => backed!.onMetricsChange(handler),
      onSideChannel: (handler) => backed!.onSideChannel(handler),
      settle: () => backed!.settle(),
      serializeAsync: (scrollbackRows) => backed!.serializeAsync(scrollbackRows),
      serializeScrollbackAsync: (maxRows) => backed!.serializeScrollbackAsync(maxRows),
      dispose: () => {
        overlay?.dispose()
        overlay = null
        // Settle in-flight async queries (serialize/selectionText) to a safe null BEFORE
        // the pane's worker slot goes away, so save/hydrate/fork awaiters in a quit-time
        // Promise.all can't hang on a queryResult that will never be sent.
        backed?.dispose()
        // Free THIS pane's engine in the shared worker, then drop the slot — the
        // manager terminates the worker only when this was the last pane.
        post({ type: 'dispose' })
        pane.release()
      }
    }
  }

  return {
    kind: initial.engine,
    term: backed.term,
    cellWidth: initial.cellWidth,
    cellHeight: initial.cellHeight,
    adapterInfo: null,
    bindPainter
  }
}
