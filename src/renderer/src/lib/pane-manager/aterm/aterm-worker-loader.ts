import { loadAterm } from './load-aterm'
import { decideAtermGpu } from './aterm-gpu-auto-policy'
import { MIN_GRID_COLS, MIN_GRID_ROWS } from './aterm-grid-size'
import { createWorkerBackedTerm, type WorkerBackedTerm } from './aterm-worker-term'
import { createAtermWorkerOverlay, type AtermWorkerOverlay } from './aterm-worker-overlay'
import { e2eConfig } from '@/lib/e2e-config'
import type { AtermPendingStrategy } from './aterm-strategy-select'
import type { AtermDrawerBuildConfig, AtermPainterBinding } from './aterm-drawer-config'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type {
  AtermWorkerMessage,
  AtermWorkerRequest,
  AtermWorkerState
} from './aterm-render-worker-protocol'

// Single-engine worker loader (plan: aterm-single-engine-worker.md). Builds the render
// worker that owns the ONLY engine for this pane + its transferred OffscreenCanvas, and
// a worker-backed `term` the controller binds to (synchronous snapshot reads + posted
// mutations). Replaces aterm-worker-mirror.ts, which ran a SECOND engine on the main
// thread purely for the sync query API — the duplicate this design removes.

/** Fetch the OS fallback faces as raw bytes for the WORKER engine (it has no
 *  window.api). CJK first — the worker's set_fallback_font RESETS the chain to it —
 *  then the script chain. Tolerant: any failure → [] (JetBrains Mono covers Latin). */
async function fetchWorkerFallbackFonts(): Promise<Uint8Array[]> {
  try {
    const { cjk, chain } = await window.api.fonts.getTerminalFallbackFonts()
    const faces: Uint8Array[] = []
    if (cjk) {
      faces.push(new Uint8Array(cjk.bytes))
    }
    for (const face of chain ?? []) {
      faces.push(new Uint8Array(face.bytes))
    }
    return faces
  } catch {
    return []
  }
}

// Cap the wait for the worker's first frame so a wedged worker can't hang pane
// creation; the worker normally posts state within a frame or two of init.
const WORKER_INIT_TIMEOUT_MS = 4000

export async function loadAtermWorkerEngine(
  config: AtermDrawerBuildConfig
): Promise<AtermPendingStrategy> {
  const { canvas, themeColors, fontPx } = config

  // Vite (renderer worker:{format:'es'}) bundles the worker from this URL.
  const worker = new Worker(new URL('./aterm-render-worker.ts', import.meta.url), {
    type: 'module'
  })
  const post = (msg: AtermWorkerRequest, transfer?: Transferable[]): void => {
    if (transfer) {
      worker.postMessage(msg, transfer)
    } else {
      worker.postMessage(msg)
    }
  }

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
  let firstResolved = false
  let resolveFirst: (state: AtermWorkerState) => void = () => undefined
  const firstState = new Promise<AtermWorkerState>((resolve) => {
    resolveFirst = resolve
  })

  worker.addEventListener('message', (event: MessageEvent<AtermWorkerMessage>) => {
    const data = event.data
    switch (data.type) {
      case 'state':
        if (!firstResolved) {
          firstResolved = true
          resolveFirst(data)
        } else {
          // Keep state fresh for sync reads + a11y even while hidden, but skip the
          // visible overlay repaint when suspended (resume re-paints the latest state).
          backed?.applyState(data)
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
        // GPU acquire failed in the worker → rebuild as CPU on the same canvas (it
        // can't be re-transferred) so the pane renders off-main instead of blank.
        if (data.phase === 'init' && !fellBackToCpuWorker) {
          fellBackToCpuWorker = true
          post({ type: 'fallback' })
        }
      // 'mouseBytes' (mouse-encode round-trip) is wired in Stage D.
    }
  })

  // Fetch the fonts BEFORE transferring the canvas: a font/asset failure here throws
  // with the canvas still intact, so loadAtermStrategy can fall back to the in-process
  // path. After transferControlToOffscreen the canvas is unusable by anything else, so
  // nothing fallible may run between the transfer and the worker taking over.
  const { fontBytes } = await loadAterm()
  const fontBytesCopy = fontBytes.slice() // copy the SHARED font so its buffer isn't detached
  const fallbackFonts = await fetchWorkerFallbackFonts()
  // Hand the canvas to the worker; from here ONLY the worker may draw to it.
  const offscreen = canvas.transferControlToOffscreen()
  post(
    {
      type: 'init',
      engine: useGpuWorker ? 'gpu' : 'cpu',
      canvas: offscreen,
      fontBytes: fontBytesCopy,
      fallbackFonts,
      // The wiring sizes the real grid + applies the user's fontPx/line-height via the
      // term's posted set_px/set_line_height/resize; start at MIN + line-height 1.
      rows: MIN_GRID_ROWS,
      cols: MIN_GRID_COLS,
      fontPx,
      lineHeight: 1,
      themeColors
    },
    [offscreen, fontBytesCopy.buffer, ...fallbackFonts.map((f) => f.buffer)]
  )

  // Wait for the worker's first frame so the controller's construction-time reads
  // (cell_width/height) are real before wireAtermPane runs. Race a timeout so a wedged
  // worker (no state, no error) can't hang pane creation forever; on timeout terminate
  // it and throw (the caller surfaces a broken pane rather than an infinite await).
  let initial: AtermWorkerState
  try {
    initial = await Promise.race([
      firstState,
      new Promise<never>((_, reject) =>
        setTimeout(
          () =>
            reject(new Error(`aterm worker first frame timed out (${WORKER_INIT_TIMEOUT_MS}ms)`)),
          WORKER_INIT_TIMEOUT_MS
        )
      )
    ])
  } catch (err) {
    worker.terminate()
    throw err
  }
  backed = createWorkerBackedTerm({ post, initial })
  // The worker owns the pane canvas, so search highlights + the link underline paint on
  // a main-thread stacked overlay driven by the snapshot (works for CPU + GPU worker).
  overlay = createAtermWorkerOverlay(canvas, () => themeColors.fg)
  overlay.paint(initial)

  const bindPainter = (_binding: AtermPainterBinding): AtermDrawStrategy => ({
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
    serializeAsync: (scrollbackRows) => backed!.serializeAsync(scrollbackRows),
    serializeScrollbackAsync: (maxRows) => backed!.serializeScrollbackAsync(maxRows),
    dispose: () => {
      overlay?.dispose()
      overlay = null
      post({ type: 'dispose' })
      worker.terminate()
    }
  })

  return {
    kind: initial.engine,
    term: backed.term,
    cellWidth: initial.cellWidth,
    cellHeight: initial.cellHeight,
    adapterInfo: null,
    bindPainter
  }
}
