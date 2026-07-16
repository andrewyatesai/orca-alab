import {
  decideAtermGpu,
  type AtermGpuDecision
} from '@/lib/pane-manager/aterm/aterm-gpu-auto-policy'
import { probeAtermGpu } from '@/lib/pane-manager/aterm/aterm-gpu-probe'

// Read-only renderer truth for the Terminal Engine settings pane: the draw path
// a NEW pane takes right now, per the same policy the pane wiring consults
// (aterm-gpu-auto-policy) — never a hardcoded claim. Runtime downgrades
// (GPU-init failure, context loss) still land individual panes on CPU, which is
// why the row's copy describes policy for new panes, not a per-pane guarantee.

export type TerminalEngineRendererStatus = {
  /** Draw path a new pane takes under the live policy. */
  path: 'gpu' | 'cpu'
  reason: AtermGpuDecision['reason']
  /** UNMASKED_RENDERER_WEBGL adapter string, when the GPU path is active and
   *  the debug-renderer extension exposed one. */
  adapter: string | null
  /** True when the default off-main-thread render worker hosts pane engines
   *  (window.__atermWorkerRender !== false). */
  workerPresentation: boolean
}

export function readTerminalEngineRendererStatus(): TerminalEngineRendererStatus {
  const decision = decideAtermGpu()
  // Cached after the pane wiring's first probe — no fresh WebGL context here.
  const probe = probeAtermGpu()
  return {
    path: decision.useGpu ? 'gpu' : 'cpu',
    reason: decision.reason,
    adapter: decision.useGpu ? probe.renderer : null,
    workerPresentation: typeof window === 'undefined' || window.__atermWorkerRender !== false
  }
}
