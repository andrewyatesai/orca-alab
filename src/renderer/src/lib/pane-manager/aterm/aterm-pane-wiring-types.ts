import type { AtermFileLinkOpener, AtermLinkProviderSource } from './aterm-link-input'
import type { AtermLinkContext } from './aterm-url-link-routing'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermPendingStrategy } from './aterm-strategy-select'
import type { AtermThemeColors } from './aterm-theme-colors'
import type {
  AtermPaneController,
  AtermPaneControllerOptions,
  AtermPaneInputSink,
  AtermPanePasteSink,
  AtermPaneResizeSink
} from './aterm-pane-controller-types'

// The pane-wiring seam types (config in, wired pane out, and the late bindings
// that survive a context-loss rebuild). Split from aterm-pane-wiring so the
// wiring file stays within the line budget as its seams grow.

/** Everything the wiring needs to turn a loaded strategy into a live pane. */
export type AtermPaneWiringConfig = {
  pending: AtermPendingStrategy
  canvas: HTMLCanvasElement
  container: HTMLElement
  /** The `.xterm` DOM wrapper (mirrors xterm's element node). */
  element: HTMLElement
  textarea: HTMLTextAreaElement
  /** Off-screen ARIA live region the draw path mirrors grid text into (a11y). */
  liveRegion: HTMLElement
  themeColors: AtermThemeColors
  inputSink: AtermPaneInputSink
  resizeSink: AtermPaneResizeSink
  pasteSink: AtermPanePasteSink
  linkContext?: AtermLinkContext
  controllerOptions?: AtermPaneControllerOptions
  /** Late-bound bindings shared across a context-loss rebuild (so the file-path/
   *  URL openers set on the old controller carry over to the CPU one). */
  shared: AtermSharedLateBindings
  /** Invoked when the draw path dies — WebGL2 context lost (GPU) or the render
   *  worker crashed — so the controller can swap this wiring out for an in-process
   *  CPU one. A worker crash passes its last serialized state (aterm replayable
   *  ANSI) so the rebuilt engine repaints instead of starting blank. */
  onContextLoss: (seedAnsi?: string) => void
}

/** Late-bound openers that survive a GPU→CPU context-loss rebuild. */
export type AtermSharedLateBindings = {
  fileLinkOpener: AtermFileLinkOpener | null
  activeLinkContext: AtermLinkContext | undefined
  /** The facade's registered xterm-style link providers (term_/task_ handles,
   *  cwd-resolved file paths); consulted where the engine reports no link. */
  linkProviderSource: AtermLinkProviderSource | null
  /** Durable spill-overlay identity (makePaneKey tabId:leafId), resolved at the
   *  controller-attach edge; held here so a context-loss rebuild re-registers
   *  the replacement engine under the same overlay key. */
  spillPaneKey: string | null
}

/** A wired, drawing pane: its public controller surface plus a teardown that
 *  drops only THIS wiring (engine + handlers + overlay) — used both by the
 *  controller's dispose and by a context-loss rebuild that swaps strategies. */
export type AtermWiredPane = {
  controller: AtermPaneController
  strategy: AtermDrawStrategy
  /** Tear down handlers + overlay + the strategy (engine/canvas context). */
  teardown: () => void
}
