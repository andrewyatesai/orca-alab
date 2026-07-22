import { attachAtermScrollInput } from './aterm-scroll-input'
import { attachAtermSelectionInput } from './aterm-selection-input'
import { attachAtermEventReportingInput } from './aterm-event-reporting-input'
import { attachAtermLinkInput } from './aterm-link-input'
import { createAtermLinkTooltip } from './aterm-link-tooltip'
import { createAtermOscLinkOpener, createAtermUrlOpener } from './aterm-url-link-routing'
import { copyAtermSelectionToClipboard } from './aterm-clipboard-copy'
import type { AtermMetrics } from './aterm-grid-reflow'
import type { AtermSharedLateBindings } from './aterm-pane-wiring'
import type { AtermPaneControllerOptions, AtermPaneInputSink } from './aterm-pane-controller-types'
import type { AtermTerminal } from './aterm_wasm.js'

type PointerInputBundleDeps = {
  canvas: HTMLCanvasElement
  textarea: HTMLTextAreaElement
  term: AtermTerminal
  /** SHARED live cell metrics (mutated in place by the grid reflow): the scroll/
   *  mouse/link handlers read it per event; syncDpr() pushes changes into the
   *  by-value selection deps. */
  metrics: AtermMetrics
  inputSink: AtermPaneInputSink
  controllerOptions?: AtermPaneControllerOptions
  /** Late-bound openers shared across a GPU→CPU rebuild. */
  shared: AtermSharedLateBindings
  getRows: () => number
  scheduleDraw: () => void
  isDisposed: () => boolean
  /** Fired after each mouse-driven selection mutation (facade onSelectionChange
   *  must not wait for PTY output). */
  onSelectionChanged?: () => void
}

/** The canvas pointer/scroll/selection/link/event-reporting input handlers for a
 *  pane, attached together. Returns each handler plus a `syncDpr` that refreshes
 *  the handlers' cached metrics after a DPI change (mirrors the old inline
 *  syncDependents). Extracted to keep aterm-pane-wiring focused. */
export function attachAtermPointerInputs({
  canvas,
  textarea,
  term,
  metrics,
  inputSink,
  controllerOptions,
  shared,
  getRows,
  scheduleDraw,
  isDisposed,
  onSelectionChanged
}: PointerInputBundleDeps): {
  selectionInput: ReturnType<typeof attachAtermSelectionInput>
  scrollInput: ReturnType<typeof attachAtermScrollInput>
  eventReportingInput: ReturnType<typeof attachAtermEventReportingInput>
  linkInput: ReturnType<typeof attachAtermLinkInput>
  linkTooltip: ReturnType<typeof createAtermLinkTooltip>
  syncDpr: () => void
} {
  // Window-space effects chrome offsets, live from the worker facade's snapshot
  // getters so a style toggle re-aims pointer math immediately; the in-process
  // engines expose no chrome getters → always 0/0 (byte-identical behavior).
  const chromeTerm = term as AtermTerminal & { chrome_pad?: number; chrome_head?: number }
  const getChrome = (): { pad: number; head: number } => ({
    pad: chromeTerm.chrome_pad ?? 0,
    head: chromeTerm.chrome_head ?? 0
  })

  const selectionDeps = {
    canvas,
    term,
    dpr: metrics.dpr,
    cellWidth: metrics.cellWidth,
    cellHeight: metrics.cellHeight,
    redraw: scheduleDraw,
    isDisposed,
    onCopy: copyAtermSelectionToClipboard,
    getCopyOnSelect: controllerOptions?.getCopyOnSelect,
    onSelectionChanged,
    getChrome
  }
  const selectionInput = attachAtermSelectionInput(selectionDeps)

  const scrollInput = attachAtermScrollInput({
    canvas,
    term,
    metrics,
    getRows,
    redraw: scheduleDraw,
    isDisposed,
    // Alt-screen wheel synthesis sends arrow presses through the same PTY seam
    // keystrokes use.
    inputSink,
    getScrollSensitivity: controllerOptions?.getScrollSensitivity,
    getFastScrollSensitivity: controllerOptions?.getFastScrollSensitivity,
    getTuiScrollMultiplier: controllerOptions?.getTuiScrollMultiplier
  })

  const eventReportingInput = attachAtermEventReportingInput({
    canvas,
    textarea,
    term,
    metrics,
    getRows,
    inputSink,
    isDisposed,
    getTuiScrollMultiplier: controllerOptions?.getTuiScrollMultiplier,
    getChrome
  })

  // URL/file-path openers are held in `shared` so a GPU→CPU rebuild keeps the
  // late-bound openers the lifecycle set on the prior controller.
  const openUrl = createAtermUrlOpener(() => shared.activeLinkContext)
  // Kind-0 OSC-8 targets get the scheme-aware router (file://, Windows paths);
  // same late-bound context, so a GPU→CPU rebuild keeps both openers aligned.
  const openOscUrl = createAtermOscLinkOpener(() => shared.activeLinkContext)

  // The hover tooltip DOM overlay (main-thread on all draw paths); the link
  // input feeds it hover/leave, and it consumes PaneManagerOptions'
  // formatLinkTooltip (e.g. localhost port worktree labels).
  const linkTooltip = createAtermLinkTooltip({
    canvas,
    textarea,
    metrics,
    isDisposed,
    formatLinkTooltip: controllerOptions?.formatLinkTooltip
  })

  const linkInput = attachAtermLinkInput({
    canvas,
    term,
    metrics,
    redraw: scheduleDraw,
    isDisposed,
    openUrl,
    openOscUrl,
    getFileLinkOpener: () => shared.fileLinkOpener,
    getLinkProviders: () => shared.linkProviderSource?.() ?? [],
    linkTooltip,
    getChrome
  })

  return {
    selectionInput,
    scrollInput,
    eventReportingInput,
    linkInput,
    linkTooltip,
    // Scroll/mouse/link read the shared `metrics` live; only the selection deps
    // still hold by-value copies, so push the new metrics into them.
    syncDpr: () => {
      selectionDeps.dpr = metrics.dpr
      selectionDeps.cellWidth = metrics.cellWidth
      selectionDeps.cellHeight = metrics.cellHeight
    }
  }
}
