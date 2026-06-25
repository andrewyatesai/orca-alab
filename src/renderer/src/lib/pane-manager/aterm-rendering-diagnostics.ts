import { useAppStore } from '@/store'
import type {
  ManagedPaneInternal,
  PaneManagerOptions,
  PaneRenderingDiagnostics
} from './pane-manager-types'
import { rebuildAtermPaneForGpuMode } from './aterm/aterm-pane-gpu-rebuild'

type GpuMode = NonNullable<PaneManagerOptions['terminalGpuAcceleration']>

/** Real per-pane renderer diagnostics from each pane's aterm controller. Reads
 *  the live draw path (gpu/cpu) + adapter; a pane whose async controller has not
 *  attached yet reports `webglAttachmentDeferred: true` and no GPU. */
export function buildPaneRenderingDiagnostics(
  panes: Iterable<ManagedPaneInternal>,
  gpuMode: GpuMode
): PaneRenderingDiagnostics[] {
  return Array.from(panes, (pane) => {
    const controller = pane.atermController
    const attached = Boolean(controller)
    const kind = controller?.rendererKind() ?? 'cpu'
    const onGpu = attached && kind === 'gpu'
    // The pane is on CPU despite the setting allowing GPU → a context-loss swap
    // (or GPU-init failure) downgraded it. 'off' never allowed GPU, so not a loss.
    const gpuAllowed = gpuMode !== 'off'
    return {
      paneId: pane.id,
      leafId: pane.leafId,
      terminalGpuAcceleration: gpuMode,
      renderer: onGpu ? 'gpu' : 'cpu',
      adapterInfo: controller?.adapterInfo() ?? null,
      hasWebgl: onGpu,
      gpuRenderingEnabled: onGpu,
      webglAttachmentDeferred: !attached,
      webglDisabledAfterContextLoss: attached && gpuAllowed && !onGpu,
      // aterm shapes complex scripts natively and never downgrades for them.
      hasComplexScriptOutput: false
    }
  })
}

/** Apply a new GPU-acceleration mode: persist it as the live setting the
 *  aterm gpu auto-policy reads (so subsequent panes honor it) and rebuild each
 *  open pane onto the matching draw path. 'auto' lets the per-host policy decide,
 *  so open panes are left as-is (rebuild only forces an explicit on/off path). */
export function applyAtermGpuMode(panes: Iterable<ManagedPaneInternal>, mode: GpuMode): void {
  const store = useAppStore.getState()
  if (store.settings && store.settings.terminalGpuAcceleration !== mode) {
    // setState is synchronous so the next pane wiring + rebuilds below read it.
    useAppStore.setState({ settings: { ...store.settings, terminalGpuAcceleration: mode } })
  }
  const desiredGpu = mode === 'on' ? true : mode === 'off' ? false : null
  for (const pane of panes) {
    pane.terminalGpuAcceleration = mode
    rebuildAtermPaneForGpuMode(pane, desiredGpu)
  }
}
