import { loadAtermCpuDrawer } from './aterm-cpu-drawer'
import { loadAtermGpuDrawer } from './aterm-gpu-drawer'
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

/** Pick + load the draw strategy for a pane. GPU is attempted when the
 *  auto-policy says so (the DEFAULT on capable hardware — see
 *  aterm-gpu-auto-policy); if GPU loading or `init` fails for any reason we fall
 *  back to the CPU drawer. So a pane ALWAYS gets a working renderer — the CPU
 *  path is the guaranteed fallback. */
export async function loadAtermStrategy(
  config: AtermDrawerBuildConfig
): Promise<AtermPendingStrategy> {
  if (isAtermGpuEnabled()) {
    try {
      const gpu = await loadAtermGpuDrawer(config)
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

  const cpu = await loadAtermCpuDrawer(config)
  return {
    kind: 'cpu',
    term: cpu.term,
    cellWidth: cpu.cellWidth,
    cellHeight: cpu.cellHeight,
    adapterInfo: null,
    bindPainter: cpu.bindPainter
  }
}
