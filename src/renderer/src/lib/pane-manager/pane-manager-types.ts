import type { IDisposable, IMarker } from '@xterm/xterm'
import type { ITerminalOptions } from '@xterm/xterm'
import type { LigaturesAddon } from '@xterm/addon-ligatures'
import type { WebglAddon } from '@xterm/addon-webgl'
import type { GlobalSettings } from '../../../../shared/types'
import type { TerminalLeafId } from '../../../../shared/stable-pane-id'
import type { TerminalWebglAutoDecision } from './terminal-webgl-auto-policy'
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
  initialRenderingSuspended?: boolean
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

export type PaneRenderingDiagnostics = {
  paneId: number
  terminalGpuAcceleration: GlobalSettings['terminalGpuAcceleration']
  gpuRenderingEnabled: boolean
  webglAttachmentDeferred: boolean
  webglDisabledAfterContextLoss: boolean
  hasComplexScriptOutput: boolean
  terminalWebglAutoDecision: TerminalWebglAutoDecision
  hasWebgl: boolean
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
  terminalGpuAcceleration: GlobalSettings['terminalGpuAcceleration']
  gpuRenderingEnabled: boolean
  webglAttachmentDeferred: boolean
  webglDisabledAfterContextLoss: boolean
  // Why: expose complex-output diagnostics without changing renderer choice;
  // auto renderer fallback is reserved for platform or WebGL failures.
  hasComplexScriptOutput: boolean
  webglAddon: WebglAddon | null
  // Why nullable: ligatures are opt-in per font and toggleable at runtime,
  // so the addon instance only exists while the feature is active. A null
  // value means "currently disabled".
  ligaturesAddon: LigaturesAddon | null
  fitResizeObserver: ResizeObserver | null
  // Stored so disposePane() can cancel the first post-open fit if a pane closes before paint.
  pendingInitialFitRafId?: number | null
  // Stored so disposePane() can cancel the post-WebGL-teardown refresh frame.
  pendingWebglRefreshRafId?: number | null
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
  debugLabel: string | null
} & ManagedPane

export type DropZone = 'top' | 'bottom' | 'left' | 'right'
