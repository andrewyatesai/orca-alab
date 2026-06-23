import type { AtermTerminal } from './aterm_wasm.js'
import { shouldForwardMouse } from './aterm-mouse-input'

export type AtermScrollDeps = {
  canvas: HTMLCanvasElement
  term: AtermTerminal
  dpr: number
  cellHeight: number
  getRows: () => number
  redraw: () => void
  isDisposed: () => boolean
}

export type AtermScrollInput = {
  dispose: () => void
}

const WHEEL_DELTA_PIXEL = 0
const WHEEL_DELTA_LINE = 1
const WHEEL_DELTA_PAGE = 2

/** Translate canvas wheel events into scrollback movement. Wheel-up reveals
 *  older lines (positive aterm delta); a fractional remainder is carried so
 *  trackpad sub-line deltas accumulate instead of being dropped. On the
 *  alternate screen we do nothing so TUIs (less, vim) handle their own wheel. */
export function attachAtermScrollInput(deps: AtermScrollDeps): AtermScrollInput {
  // dpr is read off deps live (not destructured) so a mid-session DPI change
  // (window moved to another monitor) keeps pixel-delta scrolling correct (M2).
  const { canvas, term, cellHeight, getRows, redraw, isDisposed } = deps
  let remainder = 0

  const onWheel = (event: WheelEvent): void => {
    if (isDisposed() || term.is_alt_screen) {
      return
    }
    // When a TUI tracks the mouse (no Shift), the forwarder sends wheel reports
    // to the app instead of moving scrollback; defer so we don't double-handle.
    if (shouldForwardMouse(term, event)) {
      return
    }
    event.preventDefault()

    let lines: number
    if (event.deltaMode === WHEEL_DELTA_LINE) {
      lines = event.deltaY
    } else if (event.deltaMode === WHEEL_DELTA_PAGE) {
      lines = event.deltaY * Math.max(1, getRows())
    } else {
      // WHEEL_DELTA_PIXEL: convert device pixels to grid lines.
      lines = (event.deltaY * deps.dpr) / cellHeight
    }

    remainder += lines
    const whole = Math.trunc(remainder)
    if (whole === 0) {
      return
    }
    remainder -= whole
    // Wheel down (positive deltaY) scrolls toward newer output → negative
    // aterm delta; wheel up reveals older history → positive delta.
    term.scroll_lines(-whole)
    redraw()
  }

  canvas.addEventListener('wheel', onWheel, { passive: false })

  return {
    dispose: () => {
      canvas.removeEventListener('wheel', onWheel)
    }
  }
}

export { WHEEL_DELTA_PIXEL, WHEEL_DELTA_LINE, WHEEL_DELTA_PAGE }
