import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermDrawScheduler } from './aterm-draw-scheduler'
import type { AtermEffectsDrive } from './aterm-effects-drive'
import type { AtermGridReflow } from './aterm-grid-reflow'
import type { AtermSearchMatch } from './aterm-search'

/** Owns a pane's paint: the rAF `draw` and the interactive `presentNow` fast path.
 *  Extracted from the wiring so both render identically and the wiring stays focused. */
export type AtermPanePresenter = {
  /** The draw-scheduler callback (rAF + 33ms backstop): reconcile dpr/font/line-height,
   *  then present. */
  draw: () => void
  /** Present the just-fed interactive output NOW instead of next rAF (coalesced to
   *  one paint per frame, deferred to rAF when a reconcile is pending). */
  presentNow: () => void
}

type AtermPanePresenterDeps = {
  strategy: Pick<AtermDrawStrategy, 'drawFrame'>
  /** GPU-path search overlay (null on the CPU path, which overlays on its own canvas). */
  searchOverlay: { paint: (matches: AtermSearchMatch[], activeIndex: number) => void } | null
  a11yMirror: { schedule: () => void }
  gridReflow: Pick<AtermGridReflow, 'reconcileIfNeeded'>
  drawScheduler: Pick<AtermDrawScheduler, 'consume' | 'schedule' | 'isSuspended'>
  scheduleDraw: () => void
  isDisposed: () => boolean
  getSearchMatches: () => AtermSearchMatch[]
  getSearchActiveIndex: () => number
  /** In-process effects animation drive (no-op for the worker path, whose engine
   *  ticks effects inside the worker's own frame scheduler). */
  effectsDrive: Pick<AtermEffectsDrive, 'beforeFrame' | 'afterFrame'>
}

export function createAtermPanePresenter(deps: AtermPanePresenterDeps): AtermPanePresenter {
  const {
    strategy,
    searchOverlay,
    a11yMirror,
    gridReflow,
    drawScheduler,
    scheduleDraw,
    isDisposed
  } = deps
  // True once a paint happened in the current animation frame, so presentNow
  // coalesces to one paint per frame.
  let presentedThisFrame = false

  // The actual paint: present the engine grid + overlays. Shared by the rAF draw and
  // the interactive immediate-present fast path so both render identically.
  const doPresent = (): void => {
    // Advance the clockless effects engines by the elapsed frame time BEFORE the
    // paint so this frame shows the advanced state; afterFrame keeps rAF cadence
    // only while the engine reports an active animation (idle-to-zero contract).
    deps.effectsDrive.beforeFrame()
    strategy.drawFrame()
    searchOverlay?.paint(deps.getSearchMatches(), deps.getSearchActiveIndex())
    a11yMirror.schedule()
    deps.effectsDrive.afterFrame()
  }

  const draw = (): void => {
    // A real animation frame ran: re-allow an immediate present this frame.
    presentedThisFrame = false
    // Self-heal a devicePixelRatio (or font/line-height) captured before the window
    // settled onto its real backing store (a pane created pre-Retina-attach is
    // rasterized at dpr=1 and upscales to a dpr=2 panel = blur). Every pane draws at
    // least once post-settle; the guard is a cheap compare and reconciles only on a
    // real change.
    //
    // CRITICAL (GPU path): a reconcile calls surface.configure to resize the WebGL2
    // swapchain. Presenting into it in the SAME turn makes get_current_texture()
    // return Outdated, so the frame is dropped to a black canvas; the reconcile's own
    // scheduleDraw() is swallowed while a frame is in flight, so nothing re-arms and
    // an idle shell never repaints. On reconcile, consume THIS frame and arm a clean
    // one; the present lands next rAF on the stable swapchain.
    if (gridReflow.reconcileIfNeeded()) {
      drawScheduler.consume()
      scheduleDraw()
      return
    }
    doPresent()
  }

  // Interactive fast path: a keystroke echo feeds the engine synchronously, but the
  // rAF present lands a full display refresh later (the echo arrives AFTER this
  // frame's rAF already ran), so the glyph is composited one frame late
  // (~8ms@120Hz, ~17ms@60Hz). Present NOW so it catches the current compositor frame.
  const presentNow = (): void => {
    if (isDisposed()) {
      return
    }
    // Hidden/occluded pane: don't burn an eager paint. Record the want so the
    // scheduler repaints the latest state on resume (honors draw suspension).
    if (drawScheduler.isSuspended()) {
      scheduleDraw()
      return
    }
    if (presentedThisFrame) {
      scheduleDraw() // already painted this frame; newer state coalesces onto rAF
      return
    }
    if (gridReflow.reconcileIfNeeded()) {
      // A reconcile resized the swapchain — must NOT present this turn (see draw()).
      drawScheduler.consume()
      scheduleDraw()
      return
    }
    // Mark a frame scheduled so the painter's scheduled-guard passes, then present
    // NOW; the painter consumes (cancelling the armed rAF/backstop), so the eager
    // paint never doubles up with the coalesced one.
    drawScheduler.schedule()
    doPresent()
    presentedThisFrame = true
    // Re-open the gate at the next frame so subsequent keystrokes present eagerly too.
    requestAnimationFrame(() => {
      presentedThisFrame = false
    })
  }

  return { draw, presentNow }
}
