import type { AtermTerminal } from './aterm_wasm.js'
import { encode_key_with_mode } from './aterm_wasm.js'
import { shouldForwardMouse } from './aterm-mouse-input'
import type { AtermMetrics } from './aterm-grid-reflow'
import {
  accumulateWheelLines,
  resolveScrollbackWheelSensitivity,
  resolveTuiWheelMultiplier
} from './aterm-wheel-lines'

/** Sends synthesized arrow-key bytes to the PTY — the same seam keystrokes use. */
export type AtermScrollInputSink = (data: string) => void

export type AtermScrollDeps = {
  canvas: HTMLCanvasElement
  term: AtermTerminal
  /** Shared live cell metrics (mutated in place by the grid reflow on DPI/font
   *  changes) — read per event so pixel-delta scrolling never goes stale. */
  metrics: AtermMetrics
  getRows: () => number
  redraw: () => void
  isDisposed: () => boolean
  inputSink: AtermScrollInputSink
  /** Latest scrollSensitivity (scrollback wheel lines multiplier). */
  getScrollSensitivity?: () => number
  /** Latest fastScrollSensitivity (applied while Alt is held). */
  getFastScrollSensitivity?: () => number
  /** Latest terminalTuiScrollSensitivity (alt-screen arrow synthesis multiplier). */
  getTuiScrollMultiplier?: () => number
}

export type AtermScrollInput = {
  dispose: () => void
}

/** Translate canvas wheel events into scrollback movement, or — on the
 *  alternate screen with mouse tracking off (less, man, git log) — into
 *  synthesized ArrowUp/ArrowDown presses through the ENGINE key encoder, so
 *  DECCKM/kitty forms stay exact (xterm's alternate-scroll behavior, applied
 *  unconditionally like xterm; DEC 1007 additionally requests it). Wheel-up
 *  reveals older lines (positive aterm delta); a fractional remainder is
 *  carried so trackpad sub-line deltas accumulate instead of being dropped. */
export function attachAtermScrollInput(deps: AtermScrollDeps): AtermScrollInput {
  const { canvas, term, metrics, getRows, redraw, isDisposed, inputSink } = deps
  let remainder = 0

  // The worker-backed term has no engine instance on this thread: encode via
  // the module-level encoder with the snapshot's keyboard-mode bits (one frame
  // stale, same tradeoff as is_app_cursor_mode). In-process uses the live engine.
  const encodeArrow = (key: 'ArrowUp' | 'ArrowDown'): Uint8Array | undefined =>
    typeof term.encode_key === 'function'
      ? term.encode_key(key, 0, 0, null)
      : encode_key_with_mode(key, 0, 0, null, term.keyboard_mode_bits)

  const sendArrowPresses = (lines: number): void => {
    const bytes = encodeArrow(lines > 0 ? 'ArrowDown' : 'ArrowUp')
    if (!bytes || bytes.length === 0) {
      return
    }
    // Latin-1 round-trip (see aterm-mouse-input): each byte maps 1:1 to a char
    // code so the input seam's TextEncoder re-encode keeps the bytes intact.
    inputSink(String.fromCharCode(...bytes).repeat(Math.abs(lines)))
  }

  const onWheel = (event: WheelEvent): void => {
    if (isDisposed()) {
      return
    }
    // When a TUI tracks the mouse (no Shift), the forwarder sends wheel reports
    // to the app instead of moving scrollback; defer so we don't double-handle.
    if (shouldForwardMouse(term, event)) {
      return
    }
    const altScreen = term.is_alt_screen
    // Options are read live per event: the options bag mutates on settings change.
    const sensitivity = altScreen
      ? resolveTuiWheelMultiplier(event, deps.getTuiScrollMultiplier?.() ?? 1)
      : resolveScrollbackWheelSensitivity({
          altKey: event.altKey,
          scrollSensitivity: deps.getScrollSensitivity?.() ?? 1,
          fastScrollSensitivity: deps.getFastScrollSensitivity?.() ?? 1
        })
    event.preventDefault()

    const result = accumulateWheelLines({
      deltaY: event.deltaY,
      deltaMode: event.deltaMode,
      dpr: metrics.dpr,
      cellHeight: metrics.cellHeight,
      rows: getRows(),
      sensitivity,
      remainder
    })
    remainder = result.remainder
    if (result.lines === 0) {
      return
    }
    if (altScreen) {
      // One arrow press per scrolled line; the TUI moves its own viewport.
      sendArrowPresses(result.lines)
      return
    }
    // Wheel down (positive deltaY) scrolls toward newer output → negative
    // aterm delta; wheel up reveals older history → positive delta.
    term.scroll_lines(-result.lines)
    redraw()
  }

  canvas.addEventListener('wheel', onWheel, { passive: false })

  return {
    dispose: () => {
      canvas.removeEventListener('wheel', onWheel)
    }
  }
}
