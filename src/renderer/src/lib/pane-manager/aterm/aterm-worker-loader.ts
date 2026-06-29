import { loadAterm } from './load-aterm'
import { decideAtermGpu } from './aterm-gpu-auto-policy'
import { MIN_GRID_COLS, MIN_GRID_ROWS } from './aterm-grid-size'
import { createWorkerBackedTerm, type WorkerBackedTerm } from './aterm-worker-term'
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
          backed?.applyState(data)
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
      case 'error':
        // GPU acquire failed in the worker → rebuild as CPU on the same canvas (it
        // can't be re-transferred) so the pane renders off-main instead of blank.
        if (data.phase === 'init' && !fellBackToCpuWorker) {
          fellBackToCpuWorker = true
          post({ type: 'fallback' })
        }
      // 'mouseBytes' / 'queryResult' (async round-trips) are wired in Stages C/D.
    }
  })

  // Hand the canvas to the worker; from here ONLY the worker may draw to it.
  const offscreen = canvas.transferControlToOffscreen()
  // Copy the SHARED primary font before transferring its buffer so other panes' cached
  // fontBytes isn't detached.
  const { fontBytes } = await loadAterm()
  const fontBytesCopy = fontBytes.slice()
  const fallbackFonts = await fetchWorkerFallbackFonts()
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
  // (cell_width/height) are real before wireAtermPane runs.
  const initial = await firstState
  backed = createWorkerBackedTerm({ post, initial })

  const bindPainter = (_binding: AtermPainterBinding): AtermDrawStrategy => ({
    term: backed!.term,
    getCanvas: () => canvas,
    // Search/link OVERLAY painting is wired in the overlay stage; the worker presents
    // the engine grid (incl. selection, which the engine draws) here.
    needsSearchOverlay: false,
    drawFrame: () => post({ type: 'draw' }),
    resize: (rows, cols) => backed!.term.resize(rows, cols),
    onReply: (handler) => backed!.onReply(handler),
    onMetricsChange: (handler) => backed!.onMetricsChange(handler),
    dispose: () => {
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
