import { attachAtermScrollInput } from './aterm-scroll-input'
import { attachAtermSelectionInput } from './aterm-selection-input'
import { attachAtermEventReportingInput } from './aterm-event-reporting-input'
import { attachAtermLinkInput } from './aterm-link-input'
import { createAtermUrlOpener } from './aterm-url-link-routing'
import { copyAtermSelectionToClipboard } from './aterm-clipboard-copy'
import type { AtermMetrics } from './aterm-grid-reflow'
import type { AtermSharedLateBindings } from './aterm-pane-wiring'
import type { AtermPaneControllerOptions, AtermPaneInputSink } from './aterm-pane-controller-types'
import type { AtermTerminal } from './aterm_wasm.js'

type PointerInputBundleDeps = {
  canvas: HTMLCanvasElement
  textarea: HTMLTextAreaElement
  term: AtermTerminal
  /** Live cell metrics (mutated by the grid reflow); syncDpr() pushes a DPI
   *  change into the per-handler dep objects. */
  metrics: AtermMetrics
  inputSink: AtermPaneInputSink
  controllerOptions?: AtermPaneControllerOptions
  /** Late-bound openers shared across a GPU→CPU rebuild. */
  shared: AtermSharedLateBindings
  getRows: () => number
  scheduleDraw: () => void
  isDisposed: () => boolean
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
  isDisposed
}: PointerInputBundleDeps): {
  selectionInput: ReturnType<typeof attachAtermSelectionInput>
  scrollInput: ReturnType<typeof attachAtermScrollInput>
  eventReportingInput: ReturnType<typeof attachAtermEventReportingInput>
  linkInput: ReturnType<typeof attachAtermLinkInput>
  syncDpr: () => void
} {
  const selectionDeps = {
    canvas,
    term,
    dpr: metrics.dpr,
    cellWidth: metrics.cellWidth,
    cellHeight: metrics.cellHeight,
    redraw: scheduleDraw,
    isDisposed,
    onCopy: copyAtermSelectionToClipboard,
    getCopyOnSelect: controllerOptions?.getCopyOnSelect
  }
  const selectionInput = attachAtermSelectionInput(selectionDeps)

  const scrollDeps = {
    canvas,
    term,
    dpr: metrics.dpr,
    cellHeight: metrics.cellHeight,
    getRows,
    redraw: scheduleDraw,
    isDisposed
  }
  const scrollInput = attachAtermScrollInput(scrollDeps)

  const eventReportingInput = attachAtermEventReportingInput({
    canvas,
    textarea,
    term,
    dpr: metrics.dpr,
    cellWidth: metrics.cellWidth,
    cellHeight: metrics.cellHeight,
    inputSink,
    isDisposed
  })

  // URL/file-path openers are held in `shared` so a GPU→CPU rebuild keeps the
  // late-bound openers the lifecycle set on the prior controller.
  const openUrl = createAtermUrlOpener(() => shared.activeLinkContext)

  const linkDeps = {
    canvas,
    term,
    dpr: metrics.dpr,
    cellWidth: metrics.cellWidth,
    cellHeight: metrics.cellHeight,
    redraw: scheduleDraw,
    isDisposed,
    openUrl,
    getFileLinkOpener: () => shared.fileLinkOpener
  }
  const linkInput = attachAtermLinkInput(linkDeps)

  return {
    selectionInput,
    scrollInput,
    eventReportingInput,
    linkInput,
    // Push new metrics into the live input deps after a DPR change: scroll/link
    // need only dpr; selection needs all three; event-reporting has its own setter.
    syncDpr: () => {
      selectionDeps.dpr = metrics.dpr
      selectionDeps.cellWidth = metrics.cellWidth
      selectionDeps.cellHeight = metrics.cellHeight
      scrollDeps.dpr = metrics.dpr
      linkDeps.dpr = metrics.dpr
      eventReportingInput.setDpr(metrics.dpr)
    }
  }
}
