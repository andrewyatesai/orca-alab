import type { ManagedPaneInternal } from '../pane-manager-types'
import { openAtermPane } from './aterm-pane-open'

/** Re-open a pane's aterm controller so it picks up the current GPU-acceleration
 *  setting (read live by aterm-gpu-auto-policy at wiring). The draw path (GPU vs
 *  CPU) is chosen when the controller is built, so a runtime mode change only
 *  reaches an already-open pane by rebuilding its controller.
 *
 *  Scrollback is preserved by serializing the live engine first and re-feeding it
 *  through the facade: openAtermPane attaches asynchronously and the facade
 *  buffers pre-attach feed, replaying it in order once the new controller binds.
 *  No-op for a pane whose live renderer kind already matches the desired path. */
export function rebuildAtermPaneForGpuMode(
  pane: ManagedPaneInternal,
  desiredGpu: boolean | null
): void {
  const controller = pane.atermController
  if (pane.disposed || !controller) {
    return
  }
  // null = 'auto' (the policy decides per-host); don't churn a working pane when
  // we can't say its current path is wrong.
  if (desiredGpu !== null && (controller.rendererKind() === 'gpu') === desiredGpu) {
    return
  }
  // Snapshot the full buffer (history + viewport) as replayable ANSI before
  // tearing down the engine, then re-seed the rebuilt engine with it.
  const snapshot = controller.serialize()
  controller.dispose()
  pane.atermController = null
  if (snapshot) {
    pane.terminal.__feedEngine(snapshot)
  }
  openAtermPane(pane)
}
