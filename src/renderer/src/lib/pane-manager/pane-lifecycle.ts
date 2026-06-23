import { Terminal } from '@xterm/xterm'
import type { ITerminalOptions } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { SearchAddon } from '@xterm/addon-search'
import { Unicode11Addon } from '@xterm/addon-unicode11'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { SerializeAddon } from '@xterm/addon-serialize'

import type { PaneManagerOptions, ManagedPaneInternal } from './pane-manager-types'
import type { TerminalLeafId } from '../../../../shared/stable-pane-id'
import type { DragReorderState } from './pane-drag-reorder'
import type { DragReorderCallbacks } from './pane-drag-reorder'
import { attachPaneDrag } from './pane-drag-pointer'
import { safeFit } from './pane-tree-ops'
import {
  attachPaneFitResizeObserver,
  detachPaneFitResizeObserver
} from './pane-fit-resize-observer'
import { clearPendingSplitScrollRestore } from './pane-split-scroll'
import { buildDefaultTerminalOptions } from './pane-terminal-options'
import { activateOrcaTerminalUnicodeProvider } from './pane-terminal-unicode-provider'
import { attachTerminalMouseWheelMultiplier } from './pane-terminal-mouse-wheel'
import { attachDomRendererFocusClassSync } from './pane-dom-focus-class-sync'
import {
  ENABLE_WEBGL_RENDERER,
  attachWebgl,
  cancelPendingWebglRefresh
} from './pane-webgl-renderer'
import { shouldFocusTerminalFromPanePointerDown } from './pane-pointer-focus'
import { isAtermRendererEnabled } from './aterm/aterm-renderer-flag'
import { openAtermPane } from './aterm/aterm-pane-open'

// ---------------------------------------------------------------------------
// Pane creation, terminal open/close, addon management
// ---------------------------------------------------------------------------

function getTerminalUrlOpenHint(): string {
  return navigator.userAgent.includes('Mac')
    ? 'click to open or ⇧+click for system browser'
    : 'click to open or Shift+click for system browser'
}

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

  // Build terminal options
  const userOpts = options.terminalOptions?.(id) ?? {}
  const terminalOpts: ITerminalOptions = {
    ...buildDefaultTerminalOptions(),
    ...userOpts
  }

  const terminal = new Terminal(terminalOpts)
  const fitAddon = new FitAddon()
  const searchAddon = new SearchAddon()
  const unicode11Addon = new Unicode11Addon()
  const openLinkHint = getTerminalUrlOpenHint()

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

  const webLinksAddon = new WebLinksAddon(
    options.onLinkClick ? (event, uri) => options.onLinkClick!(event, uri) : undefined,
    {
      hover: (_event, uri) => {
        if (uri) {
          linkTooltip.textContent = `${uri} (${openLinkHint})`
          linkTooltip.style.display = ''
        }
      },
      leave: () => {
        linkTooltip.style.display = 'none'
      }
    }
  )

  const serializeAddon = new SerializeAddon()

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
    gpuRenderingEnabled: ENABLE_WEBGL_RENDERER,
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
    unicode11Addon,
    webLinksAddon,
    webglAddon: null,
    ligaturesAddon: null,
    panePointerDownHandler,
    paneMouseEnterHandler,
    paneDragCleanup,
    compositionHandler: null,
    focusClassSyncCleanup: null,
    pendingSplitScrollState: null,
    pendingSplitScrollRafIds: [],
    pendingSplitScrollTimerId: null,
    pendingSplitScrollBufferDisposable: null,
    debugLabel: options.debugLabel ?? null
  }

  // Focus handler: clicking a pane makes it active and explicitly focuses
  // the terminal. We must call focus: true here because after DOM reparenting
  // (e.g. splitPane moves the original pane into a flex container), xterm.js's
  // native click-to-focus on its internal textarea may not fire reliably.
  container.addEventListener('pointerdown', panePointerDownHandler)

  // Focus-follows-mouse handler: when the setting is enabled, hovering a
  // pane makes it active. All gating (feature flag, drag-in-progress,
  // window focus, etc.) lives in the PaneManager callback — this layer
  // just forwards the event.
  container.addEventListener('mouseenter', paneMouseEnterHandler)

  return pane
}

/** Open terminal into its container and load addons. Must be called after the container is in the DOM. */
export function openTerminal(pane: ManagedPaneInternal): void {
  // Experimental: hand painting + sizing to the in-page aterm canvas renderer.
  // xterm stays unopened (no DOM/draws) so the rest of the app keeps its
  // cols/rows/write/onData/buffer/serialize, while the canvas owns the pixels.
  if (isAtermRendererEnabled()) {
    try {
      // Why: serialize()/restore must keep working on the headless (unopened)
      // xterm Terminal. SerializeAddon + Unicode11Addon are buffer-only and
      // work without terminal.open(); load them here (the open path is skipped)
      // so snapshot/scrollback restore stays intact under default-on.
      loadBufferOnlyAddons(pane)
      // Width tables BEFORE any write — see the activate call's comment in
      // openXtermRenderer for why this must precede caller-driven writes.
      activateOrcaTerminalUnicodeProvider(pane.terminal)
      // Async wasm/font init failure → transparently become a normal xterm pane
      // (never a black pane). A sync failure falls through to the catch below.
      openAtermPane(pane, () => openXtermRenderer(pane))
      return
    } catch (err) {
      console.warn('[aterm] sync init failed for pane; falling back to xterm', pane.id, err)
      openXtermRenderer(pane)
      return
    }
  }

  openXtermRenderer(pane)
}

/** Load the buffer-only addons (serialize + unicode11) that keep terminal
 *  serialize/restore and wide-char width tables working on a headless terminal.
 *  Guarded so the xterm fallback path does not re-load them (xterm.loadAddon
 *  throws on the same instance twice). */
function loadBufferOnlyAddons(pane: ManagedPaneInternal): void {
  if (pane.bufferAddonsLoaded) {
    return
  }
  pane.terminal.loadAddon(pane.serializeAddon)
  pane.terminal.loadAddon(pane.unicode11Addon)
  pane.bufferAddonsLoaded = true
}

/** Open xterm into the DOM and load the DOM-renderer addons (fit/search/links/
 *  webgl) plus the DOM-specific wiring. This is both the default rendering path
 *  and the safe fallback when aterm init fails — so any aterm failure
 *  transparently becomes a normal xterm pane instead of a black pane. */
export function openXtermRenderer(pane: ManagedPaneInternal): void {
  const {
    terminal,
    xtermContainer,
    linkTooltip,
    terminalTuiScrollSensitivity,
    fitAddon,
    searchAddon,
    webLinksAddon
  } = pane

  // Why: a failed aterm init can leave its DOM shim appended to xtermContainer
  // — the whole `.xterm` wrapper buildAtermInputDom created (wrapper >
  // .xterm-screen > canvas + .xterm-helpers > textarea), added before the async
  // wasm load that rejected. Remove the ENTIRE aterm-built subtree so
  // terminal.open() builds a clean xterm pane and no orphan textarea/screen/
  // helpers are stranded. Prefer the `.xterm` wrapper, fall back to the
  // `.xterm-screen` (which still parents the helpers+textarea), and only as a
  // last resort drop the bare canvas — but then also sweep any sibling helpers.
  for (const canvas of xtermContainer.querySelectorAll('[data-testid="aterm-canvas"]')) {
    const atermWrapper = canvas.closest('.xterm')
    const atermScreen = canvas.closest('.xterm-screen')
    if (atermWrapper && xtermContainer.contains(atermWrapper)) {
      atermWrapper.remove()
    } else if (atermScreen && xtermContainer.contains(atermScreen)) {
      // No wrapper, but the screen still parents canvas + helpers + textarea.
      atermScreen.remove()
    } else {
      // Bare canvas (no wrapper/screen): drop it AND any sibling helpers so the
      // shim's textarea can't survive as an orphan in the open container.
      canvas.parentElement?.querySelectorAll('.xterm-helpers').forEach((el) => el.remove())
      canvas.remove()
    }
  }

  // Open terminal into DOM
  terminal.open(xtermContainer)
  const linkTooltipContainer = terminal.element ?? xtermContainer
  linkTooltipContainer.appendChild(linkTooltip)

  // Load addons (order matters: WebGL must be after open())
  terminal.loadAddon(fitAddon)
  terminal.loadAddon(searchAddon)
  // Why: the aterm branch may have already loaded serialize/unicode11 (to keep
  // serialize alive on the headless terminal). xterm.loadAddon throws on the
  // same instance twice, so only load them here when they aren't loaded yet.
  loadBufferOnlyAddons(pane)
  terminal.loadAddon(webLinksAddon)
  attachTerminalMouseWheelMultiplier(terminal, {
    getTuiMouseWheelMultiplier: terminalTuiScrollSensitivity
  })

  // Activate Orca's Unicode 11 width shim *before* any caller-driven write. CJK / emoji /
  // ZWJ codepoints get baked into the buffer at the active unicode version on
  // write — if a restore (snapshot, scrollback, cold-restore) writes bytes
  // through xterm while the default v6 width tables are still active, wide
  // chars lay out as single cells and any subsequent re-measurement breaks
  // pairing (visible as broken `?`-style glyphs). All restore paths
  // (replayTerminalLayout → splitPane/createInitialPane → openTerminal,
  // restoreScrollbackBuffers, handleReattachResult) run after openTerminal,
  // so the activation must stay at this position. Idempotent: re-activating in
  // the aterm-fallback case is a no-op for the buffer the aterm branch seeded.
  activateOrcaTerminalUnicodeProvider(terminal)

  // Why: the OS reads the focused textarea's screen rect at compositionstart to
  // decide where to display the IME candidate window. xterm.js only repositions
  // the textarea on compositionupdate (via updateCompositionElements), not on
  // compositionstart, so the window can appear at a stale cursor position. We
  // force-sync the textarea position in a capture-phase listener so the OS sees
  // the correct location before it opens the candidate window.
  //
  // Cell dimensions are derived from the public .xterm-screen element's bounds
  // (xterm sizes that element to cols*cellWidth × rows*cellHeight) rather than
  // poking `_core._renderService.dimensions` — keeps us on the public API
  // surface so upgrades don't silently regress the fix.
  if (terminal.element && terminal.textarea) {
    const screenElement = terminal.element.querySelector<HTMLElement>('.xterm-screen')
    const textarea = terminal.textarea
    const handler = (): void => {
      if (!screenElement) {
        return
      }
      const rect = screenElement.getBoundingClientRect()
      const cellWidth = rect.width / terminal.cols
      const cellHeight = rect.height / terminal.rows
      if (!(cellWidth > 0) || !(cellHeight > 0)) {
        return
      }
      const buf = terminal.buffer.active
      const x = Math.min(buf.cursorX, terminal.cols - 1)
      textarea.style.top = `${buf.cursorY * cellHeight}px`
      textarea.style.left = `${x * cellWidth}px`
    }
    terminal.element.addEventListener('compositionstart', handler, true)
    // Store so disposePane() can remove it and avoid a memory leak.
    pane.compositionHandler = handler
  }

  pane.focusClassSyncCleanup = attachDomRendererFocusClassSync(terminal.element)

  if (pane.gpuRenderingEnabled) {
    attachWebgl(pane)
  }

  attachPaneFitResizeObserver(pane)

  // Initial fit (deferred to ensure layout has settled)
  if (pane.pendingInitialFitRafId != null) {
    cancelAnimationFrame(pane.pendingInitialFitRafId)
  }
  pane.pendingInitialFitRafId = requestAnimationFrame(() => {
    pane.pendingInitialFitRafId = null
    safeFit(pane)
  })
}

export function disposePane(
  pane: ManagedPaneInternal,
  panes: Map<number, ManagedPaneInternal>
): void {
  // Mark first so an in-flight async aterm controller creation drops its result.
  // controller.dispose() is internally guarded, so no try/catch is needed here.
  pane.disposed = true
  pane.atermMirrorCleanup?.()
  pane.atermMirrorCleanup = null
  pane.atermController?.dispose()
  pane.atermController = null
  if (pane.pendingInitialFitRafId != null) {
    cancelAnimationFrame(pane.pendingInitialFitRafId)
    pane.pendingInitialFitRafId = null
  }
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
  pane.focusClassSyncCleanup?.()
  pane.focusClassSyncCleanup = null
  if (pane.compositionHandler) {
    pane.terminal.element?.removeEventListener('compositionstart', pane.compositionHandler, true)
    pane.compositionHandler = null
  }
  try {
    clearPendingSplitScrollRestore(pane)
  } catch {
    /* ignore */
  }
  try {
    pane.ligaturesAddon?.dispose()
  } catch {
    /* ignore */
  }
  try {
    pane.webglAddon?.dispose()
  } catch {
    /* ignore */
  }
  try {
    pane.searchAddon.dispose()
  } catch {
    /* ignore */
  }
  try {
    pane.serializeAddon.dispose()
  } catch {
    /* ignore */
  }
  try {
    pane.unicode11Addon.dispose()
  } catch {
    /* ignore */
  }
  try {
    pane.webLinksAddon.dispose()
  } catch {
    /* ignore */
  }
  try {
    pane.fitAddon.dispose()
  } catch {
    /* ignore */
  }
  try {
    pane.terminal.dispose()
  } catch {
    /* ignore */
  }
  panes.delete(pane.id)
}
