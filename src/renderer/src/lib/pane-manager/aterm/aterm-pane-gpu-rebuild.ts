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
export async function rebuildAtermPaneForGpuMode(
  pane: ManagedPaneInternal,
  desiredGpu: boolean | null
): Promise<void> {
  const controller = pane.atermController
  if (pane.disposed || !controller) {
    return
  }
  // null = 'auto': the per-host policy already chose this pane's path, so leave a
  // working pane untouched. For explicit on/off, skip if the live path matches.
  if (desiredGpu === null || (controller.rendererKind() === 'gpu') === desiredGpu) {
    return
  }
  // Snapshot the full buffer (history + viewport) as replayable ANSI before tearing
  // down the engine. MUST be the awaitable serialize: on the single-engine worker path
  // the sync serialize() returns only the debounced/stale (usually empty) cache, so the
  // rebuilt engine would re-seed from nothing and wipe the pane -- serializeAsync
  // round-trips to the worker for the fresh full history (cost-free in-process).
  const snapshot = await controller.serializeAsync()
  // The await yielded; bail if the pane was torn down or already rebuilt meanwhile so we
  // don't dispose/replace a controller that is no longer the live one.
  if (pane.disposed || pane.atermController !== controller) {
    return
  }
  controller.dispose()
  pane.atermController = null
  if (snapshot) {
    pane.terminal.__feedEngine(snapshot)
  }
  openAtermPane(pane)
}
