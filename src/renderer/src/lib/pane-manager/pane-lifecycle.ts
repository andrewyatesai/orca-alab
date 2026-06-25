import type { PaneManagerOptions, ManagedPaneInternal } from './pane-manager-types'
import type { TerminalLeafId } from '../../../../shared/stable-pane-id'
import type { DragReorderState } from './pane-drag-reorder'
import type { DragReorderCallbacks } from './pane-drag-reorder'
import { attachPaneDrag } from './pane-drag-pointer'
import { detachPaneFitResizeObserver } from './pane-fit-resize-observer'
import { clearPendingSplitScrollRestore } from './pane-split-scroll'
import { cancelPendingWebglRefresh } from './pane-webgl-renderer'
import { shouldFocusTerminalFromPanePointerDown } from './pane-pointer-focus'
import { openAtermPane } from './aterm/aterm-pane-open'
import { createAtermTerminalFacade } from './aterm/aterm-terminal-facade'
import {
  createAtermFitAddonFacade,
  createAtermSearchAddonFacade,
  createAtermSerializeAddonFacade
} from './aterm/aterm-addon-facades'
import { buildDefaultTerminalOptions } from './pane-terminal-options'

// ---------------------------------------------------------------------------
// Pane creation, terminal open/close, addon management
// ---------------------------------------------------------------------------

export function createPaneDOM(
  id: number,
  leafId: TerminalLeafId,
  options: PaneManagerOptions,
  dragState: DragReorderState,
  dragCallbacks: DragReorderCallbacks,
  onPointerDown: (id: number, options?: { focusTerminal?: boolean }) => void,
  onMouseEnter: (id: number, event: MouseEvent) => void
): ManagedPaneInternal {
  // Create .pane container
  const container = document.createElement('div')
  container.className = 'pane'
  container.dataset.paneId = String(id)
  container.dataset.leafId = leafId

  // Create .xterm-container — baseline layout (position, width, height, margin)
  // is CSS-driven (see main.css .xterm-container) so that the data-has-title
  // attribute override can shift the terminal down without racing safeFit().
  const xtermContainer = document.createElement('div')
  xtermContainer.className = 'xterm-container'
  container.appendChild(xtermContainer)

  // The pane terminal is an aterm-backed facade: the engine (via the async
  // controller) owns pixels/parsing/serialize/search, while the ~46 consumers
  // keep the xterm-shaped cols/rows/write/buffer/parser/onData surface unchanged.
  const userOpts = options.terminalOptions?.(id) ?? {}
  const terminal = createAtermTerminalFacade({
    options: { ...buildDefaultTerminalOptions(), ...userOpts }
  })
  // Thin addon facades over the same controller (FitAddon/SearchAddon/Serialize
  // Addon replacements). The controller is read live; null before async attach.
  const getController = (): ManagedPaneInternal['atermController'] => pane.atermController
  const fitAddon = createAtermFitAddonFacade(getController)
  const searchAddon = createAtermSearchAddonFacade(getController)
  const serializeAddon = createAtermSerializeAddonFacade(getController)

  // URL tooltip element — Ghostty-style bottom-left hint on hover
  const linkTooltip = document.createElement('div')
  linkTooltip.className = 'pane-link-tooltip'
  linkTooltip.classList.add('xterm-hover')
  linkTooltip.style.cssText =
    'display:none;position:absolute;bottom:4px;left:8px;z-index:40;' +
    'padding:5px 8px;border-radius:4px;font-size:11px;font-family:inherit;' +
    'color:#a1a1aa;background:rgba(24,24,27,0.85);border:1px solid rgba(63,63,70,0.6);' +
    'pointer-events:none;max-width:80%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;'

  // Ghostty-style drag handle — appears at top of pane on hover when 2+ panes
  const dragHandle = document.createElement('div')
  dragHandle.className = 'pane-drag-handle'
  container.appendChild(dragHandle)
  const paneDragCleanup = attachPaneDrag(dragHandle, id, dragState, dragCallbacks)

  const panePointerDownHandler = (event: PointerEvent): void => {
    onPointerDown(id, {
      focusTerminal: shouldFocusTerminalFromPanePointerDown(event.target)
    })
  }

  const paneMouseEnterHandler = (event: MouseEvent): void => {
    onMouseEnter(id, event)
  }

  const pane: ManagedPaneInternal = {
    id,
    leafId,
    stablePaneId: leafId,
    terminal,
    container,
    xtermContainer,
    linkTooltip,
    terminalTuiScrollSensitivity: options.terminalTuiScrollSensitivity,
    terminalGpuAcceleration: options.terminalGpuAcceleration ?? 'auto',
    // The aterm controller owns GPU acceleration via its own draw strategy; the
    // xterm-WebGL/ligatures paths stay disabled (they targeted the deleted DOM
    // renderer), so these xterm-only fields are inert for aterm panes.
    gpuRenderingEnabled: false,
    webglAttachmentDeferred: false,
    webglDisabledAfterContextLoss: false,
    hasComplexScriptOutput: false,
    fitAddon,
    fitResizeObserver: null,
    pendingInitialFitRafId: null,
    pendingWebglRefreshRafId: null,
    pendingObservedFitRafId: null,
    searchAddon,
    serializeAddon,
    webglAddon: null,
    ligaturesAddon: null,
    panePointerDownHandler,
    paneMouseEnterHandler,
    paneDragCleanup,
    pendingSplitScrollState: null,
    pendingSplitScrollRafIds: [],
    pendingSplitScrollTimerId: null,
    pendingSplitScrollBufferDisposable: null,
    debugLabel: options.debugLabel ?? null
  }

  // Focus handler: clicking a pane makes it active and explicitly focuses the
  // terminal. focus: true is required because after DOM reparenting (splitPane)
  // the aterm helper-textarea's native click-to-focus may not fire reliably.
  container.addEventListener('pointerdown', panePointerDownHandler)

  // Focus-follows-mouse handler: when the setting is enabled, hovering a pane
  // makes it active. All gating lives in the PaneManager callback.
  container.addEventListener('mouseenter', paneMouseEnterHandler)

  return pane
}

/** Open the pane terminal: hand painting + sizing to the in-page aterm canvas
 *  renderer. The facade (pane.terminal) buffers any output until the async
 *  controller attaches, so callers can write/registerOscHandler immediately. */
export function openTerminal(pane: ManagedPaneInternal): void {
  openAtermPane(pane)
}

export function disposePane(
  pane: ManagedPaneInternal,
  panes: Map<number, ManagedPaneInternal>
): void {
  // Mark first so an in-flight async aterm controller creation drops its result.
  pane.disposed = true
  pane.atermController?.dispose()
  pane.atermController = null
  if (pane.pendingInitialFitRafId != null) {
    cancelAnimationFrame(pane.pendingInitialFitRafId)
    pane.pendingInitialFitRafId = null
  }
  // Cancel a leaked WebGL-refresh frame (xterm-WebGL field, inert for aterm but
  // still cleaned up if ever set).
  cancelPendingWebglRefresh(pane)
  detachPaneFitResizeObserver(pane)
  if (pane.panePointerDownHandler) {
    pane.container.removeEventListener('pointerdown', pane.panePointerDownHandler)
    pane.panePointerDownHandler = null
  }
  if (pane.paneMouseEnterHandler) {
    pane.container.removeEventListener('mouseenter', pane.paneMouseEnterHandler)
    pane.paneMouseEnterHandler = null
  }
  pane.paneDragCleanup?.()
  pane.paneDragCleanup = null
  try {
    clearPendingSplitScrollRestore(pane)
  } catch {
    /* ignore */
  }
  try {
    // Disposes the facade, which tears down the controller (engine + handlers +
    // overlays + DOM) — the single owner of aterm pane teardown now.
    pane.terminal.dispose()
  } catch {
    /* ignore */
  }
  panes.delete(pane.id)
}
