import { loadAtermCpuDrawer } from './aterm-cpu-drawer'
import { loadAtermGpuDrawer } from './aterm-gpu-drawer'
import { loadAtermWorkerEngine } from './aterm-worker-loader'
import { isAtermGpuEnabled } from './aterm-gpu-auto-policy'
import { probeAtermGpu } from './aterm-gpu-probe'
import { e2eConfig } from '@/lib/e2e-config'
import type { AtermDrawerBuildConfig, AtermPainterBinding } from './aterm-drawer-config'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermTerminal } from './aterm_wasm.js'

/** The chosen, loaded-but-not-yet-painting drawer. `kind` lets the controller
 *  wire GPU-only extras (the search overlay + the e2e offscreen hook). */
export type AtermPendingStrategy = {
  kind: 'cpu' | 'gpu'
  term: AtermTerminal
  cellWidth: number
  cellHeight: number
  /** GPU only: the acquired WebGL adapter/backend string (else null). */
  adapterInfo: string | null
  bindPainter: (binding: AtermPainterBinding) => AtermDrawStrategy
}

// A broken/half-initialized GPU stack (software-GL passthrough, RDP, a wedged
// driver) can leave the async adapter/surface acquire HANGING rather than
// rejecting — `init` never settles and the pane would stay blank forever. Cap the
// GPU init so we fall through to the guaranteed CPU path instead.
const GPU_INIT_TIMEOUT_MS = 4000

/** Pick + load the draw strategy for a pane. GPU is attempted when the
 *  auto-policy says so (the DEFAULT on capable hardware — see
 *  aterm-gpu-auto-policy); if GPU loading or `init` fails OR HANGS we fall back to
 *  the CPU drawer. So a pane ALWAYS gets a working renderer — the CPU path is the
 *  guaranteed fallback. */
export async function loadAtermStrategy(
  config: AtermDrawerBuildConfig
): Promise<AtermPendingStrategy> {
  // DEFAULT single-engine worker: the ONLY engine lives in a worker that owns the
  // OffscreenCanvas (parse + render off the main thread), so heavy terminal output never
  // competes with the renderer main thread; the controller binds to a snapshot-backed
  // `term`. The worker handles GPU→CPU internally; if it can't even post a first frame
  // (or fonts fail before the canvas transfer) we fall back to the in-process CPU/GPU
  // path on the still-intact canvas (loadAtermWorkerEngine fetches fonts before the
  // transfer + races a first-frame timeout). Set `window.__atermWorkerRender = false`
  // to opt out (the e2e suite does this so its in-process canvas/GPU assertions still
  // hold; the dedicated worker specs opt back in with `= true`).
  // The in-process fallback may need a FRESH canvas: if the worker attempt got as far
  // as transferControlToOffscreen the original is poisoned (getContext throws), so a
  // CPU/GPU load on it would reject and leave the pane permanently blank.
  let cfg = config
  if (typeof window === 'undefined' || window.__atermWorkerRender !== false) {
    try {
      return await loadAtermWorkerEngine(config)
    } catch (err) {
      // Worker couldn't init (a wedged first frame, fonts failed before the transfer,
      // or the first-frame timeout). Rebuild a fresh, un-transferred canvas so the
      // in-process path has a usable surface; without it the fallback dies on the dead
      // canvas and the pane stays blank — strictly worse than slow.
      console.warn('[aterm] off-main worker init failed; falling back to in-process', err)
      if (config.rebuildCanvas) {
        cfg = { ...config, canvas: config.rebuildCanvas() }
      }
    }
  }

  if (isAtermGpuEnabled()) {
    try {
      // Race init against a timeout so a hung adapter acquire can't wedge the pane;
      // if the GPU drawer resolves AFTER we've timed out, free its orphaned engine.
      const gpuPromise = loadAtermGpuDrawer(cfg)
      let timedOut = false
      void gpuPromise
        .then((late) => {
          if (timedOut) {
            try {
              late.term.free()
            } catch {
              /* ignore */
            }
          }
        })
        .catch(() => undefined)
      const gpu = await Promise.race([
        gpuPromise,
        new Promise<never>((_, reject) =>
          setTimeout(() => {
            timedOut = true
            reject(new Error(`GPU init exceeded ${GPU_INIT_TIMEOUT_MS}ms`))
          }, GPU_INIT_TIMEOUT_MS)
        )
      ])
      return {
        kind: 'gpu',
        term: gpu.term,
        cellWidth: gpu.cellWidth,
        cellHeight: gpu.cellHeight,
        adapterInfo: gpu.adapterInfo,
        bindPainter: gpu.bindPainter
      }
    } catch (err) {
      // WebGL init failed (unavailable / surface / adapter) — fall back to CPU.
      const probe = probeAtermGpu()
      const reason = err instanceof Error ? err.message : String(err)
      console.warn(
        '[aterm] GPU draw path init failed; falling back to CPU',
        { renderer: probe.renderer, vendor: probe.vendor },
        err
      )
      // e2e only: surface the failure reason so the WebGL spec can report WHY the
      // GPU path didn't engage instead of just observing a CPU canvas.
      if (e2eConfig.exposeStore) {
        window.__atermGpuFailureReason = reason
      }
    }
  }

  const cpu = await loadAtermCpuDrawer(cfg)
  return {
    kind: 'cpu',
    term: cpu.term,
    cellWidth: cpu.cellWidth,
    cellHeight: cpu.cellHeight,
    adapterInfo: null,
    bindPainter: cpu.bindPainter
  }
}
