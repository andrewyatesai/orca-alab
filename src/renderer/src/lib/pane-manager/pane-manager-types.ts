import type { IDisposable, IMarker } from './aterm/terminal-types'
import type { ITerminalOptions } from './aterm/terminal-types'
import type { GlobalSettings } from '../../../../shared/types'
import type { TerminalLeafId } from '../../../../shared/stable-pane-id'
import type { AtermPaneController } from './aterm/aterm-pane-renderer'
import type { AtermTerminalFacade } from './aterm/aterm-terminal-facade'
import type {
  AtermFitAddonFacade,
  AtermSearchAddonFacade,
  AtermSerializeAddonFacade
} from './aterm/aterm-addon-facades'

// ---------------------------------------------------------------------------
// Public interfaces
// ---------------------------------------------------------------------------

/** Hints forwarded from splitPane() into onPaneCreated for a single split.
 *  Carries one-shot PTY spawn/adoption data for the new pane.
 *  Kept as a separate parameter (rather than extending ManagedPane) so the
 *  hint is scoped to pane creation and does not live on the pane afterwards. */
export type PaneSpawnHints = {
  cwd?: string
  ptyId?: string
}

export type ClosedPaneInfo = {
  paneId: number
  leafId: TerminalLeafId
}

export type PaneManagerOptions = {
  onPaneCreated?: (pane: ManagedPane, spawnHints?: PaneSpawnHints) => void | Promise<void>
  onPaneClosed?: (paneId: number, closedPane?: ClosedPaneInfo) => void
  onActivePaneChange?: (pane: ManagedPane) => void
  onLayoutChanged?: () => void
  /** Why: Electron webviews can steal pointer streams from renderer-owned
   *  pane drags unless callers temporarily put them in pointer passthrough. */
  onPaneDragActiveChange?: (active: boolean) => void
  terminalOptions?: (paneId: number) => Partial<ITerminalOptions>
  terminalTuiScrollSensitivity?: () => number | undefined
  onLinkClick?: (event: MouseEvent | undefined, url: string) => void
  terminalGpuAcceleration?: GlobalSettings['terminalGpuAcceleration']
  // Why: diagnostic label for log correlation. safeFit and other internal
  // helpers log warnings that are hard to correlate without knowing which
  // tab/worktree the PaneManager belongs to.
  debugLabel?: string
}

export type PaneStyleOptions = {
  splitBackground?: string
  paneBackground?: string
  inactivePaneOpacity?: number
  activePaneOpacity?: number
  opacityTransitionMs?: number
  dividerThicknessPx?: number
  // Why this behavior flag lives on "style" options: this type is already
  // the single runtime-settings bag the PaneManager exposes. Splitting into
  // separate style vs behavior types is a refactor worth its own change
  // when a second behavior flag lands. See docs/focus-follows-mouse-design.md.
  focusFollowsMouse?: boolean
  paddingX?: number
  paddingY?: number
}

export type ManagedPane = {
  id: number
  /** Durable terminal layout leaf UUID. Use this for paneKey/ORCA_PANE_KEY and
   *  persisted leaf-keyed state; `id` is only the live renderer handle. */
  leafId: TerminalLeafId
  /** Compatibility alias while callers migrate from the older stablePaneId name. */
  stablePaneId: TerminalLeafId
  // The pane terminal: an aterm-backed facade with the xterm-Terminal-shaped
  // surface orca's consumers use (cols/rows/buffer/parser/write/onData/etc.).
  terminal: AtermTerminalFacade
  container: HTMLElement // the .pane element
  linkTooltip: HTMLElement
  fitAddon: AtermFitAddonFacade
  searchAddon: AtermSearchAddonFacade
  serializeAddon: AtermSerializeAddonFacade
  // Present only when the experimental aterm canvas renderer owns painting and
  // sizing for this pane; xterm stays unopened but keeps buffer/serialize state.
  atermController?: AtermPaneController | null
  // The input→PTY router installed by connectPanePty: aterm's input sink calls this
  // to drive keystrokes/paste/drained-replies through the full intent/presence/replay
  // pipeline WITHOUT going through the xterm shim's Terminal.input(). Undefined until
  // the pane's PTY is connected (before that, the sink falls back to xterm.input).
  routePtyInput?: (data: string) => void
  // The resize→PTY router installed by connectPanePty (presence/held-resize gates);
  // aterm's resize sink calls it directly so resize doesn't go through the xterm
  // shim. Undefined until the PTY is connected (sink falls back to xterm.resize).
  routePtyResize?: (cols: number, rows: number) => void
}

/** Real per-pane renderer diagnostics, sourced from each pane's aterm
 *  controller (its loaded draw path + adapter). The legacy xterm-WebGL field
 *  names are kept so existing consumers keep reading the same keys, but they now
 *  map to honest aterm state:
 *   - `renderer`/`adapterInfo` — the live draw path ('gpu' = WebGL2 drawer, 'cpu'
 *     = the 2d drawer / context-loss fallback) and its acquired adapter string.
 *   - `hasWebgl`/`gpuRenderingEnabled` — true iff this pane is on the GPU path.
 *   - `webglDisabledAfterContextLoss` — true iff the pane started on GPU (setting
 *     allows it) but is now on CPU, i.e. a context-loss swap occurred.
 *   - `webglAttachmentDeferred` — true while the async controller has not attached
 *     yet (no live draw path to report).
 *   - `hasComplexScriptOutput` — always false: aterm shapes complex scripts
 *     natively and never downgrades the renderer for them. */
export type PaneRenderingDiagnostics = {
  paneId: number
  /** Compatibility alias the older xterm specs read; equals `paneId`. */
  leafId?: TerminalLeafId
  terminalGpuAcceleration: GlobalSettings['terminalGpuAcceleration']
  renderer: 'gpu' | 'cpu'
  adapterInfo: string | null
  hasWebgl: boolean
  gpuRenderingEnabled: boolean
  webglAttachmentDeferred: boolean
  webglDisabledAfterContextLoss: boolean
  hasComplexScriptOutput: boolean
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

export type ScrollState = {
  bufferType: 'normal' | 'alternate'
  wasAtBottom: boolean
  viewportY: number
  baseY: number
  firstVisibleLineMarker?: IMarker
}

export type ManagedPaneInternal = {
  xtermContainer: HTMLElement
  linkTooltip: HTMLElement
  terminalTuiScrollSensitivity?: () => number | undefined
  // Read by aterm-gpu-auto-policy at wiring time to pick the GPU vs CPU drawer.
  terminalGpuAcceleration: GlobalSettings['terminalGpuAcceleration']
  fitResizeObserver: ResizeObserver | null
  // Stored so disposePane() can cancel the first post-open fit if a pane closes before paint.
  pendingInitialFitRafId?: number | null
  pendingObservedFitRafId: number | null
  serializeAddon: AtermSerializeAddonFacade
  // Stored so disposePane() can remove pane-local DOM listeners explicitly.
  panePointerDownHandler?: ((event: PointerEvent) => void) | null
  paneMouseEnterHandler?: ((event: MouseEvent) => void) | null
  paneDragCleanup?: (() => void) | null
  // Why: splitPane reparents DOM; its delayed restore owns scroll until the
  // browser settles, so intermediate fits must not compete with it.
  pendingSplitScrollState: ScrollState | null
  // Stored so repeated split restores and disposePane() can cancel deferred
  // restore handles instead of leaving stale pane closures alive.
  pendingSplitScrollRafIds?: number[]
  pendingSplitScrollTimerId?: ReturnType<typeof setTimeout> | null
  // Stored so repeated split restores and disposePane() can remove the
  // deferred alt-screen buffer listener instead of stacking callbacks.
  pendingSplitScrollBufferDisposable?: IDisposable | null
  // Set by disposePane so an in-flight async aterm controller creation can drop
  // its result instead of attaching a canvas to a torn-down pane.
  disposed?: boolean
  // Set when the pane is created while its manager's rendering is suspended; the
  // aterm controller starts with draw scheduling paused once it attaches so a
  // hidden/background manager's panes paint no frames until resumeRendering().
  startRenderingSuspended?: boolean
  debugLabel: string | null
} & ManagedPane

export type DropZone = 'top' | 'bottom' | 'left' | 'right'
