import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermMetrics } from './aterm-grid-reflow'
import { accumulateWheelLines, resolveTuiWheelMultiplier } from './aterm-wheel-lines'

/** Sends encoded PTY bytes (mouse reports) to the child. Same seam selection
 *  copy / key encoding use — the controller threads pane.terminal.input here. */
export type AtermMouseInputSink = (data: string) => void

export type AtermMouseDeps = {
  canvas: HTMLCanvasElement
  term: AtermTerminal
  /** Shared live cell metrics (mutated in place by the grid reflow on DPI/font
   *  changes) — read per event so report hit-testing never goes stale. */
  metrics: AtermMetrics
  /** Viewport rows (page-mode wheel scaling). */
  getRows: () => number
  inputSink: AtermMouseInputSink
  isDisposed: () => boolean
  /** Latest terminalTuiScrollSensitivity (wheel-report count multiplier). */
  getTuiScrollMultiplier?: () => number
  /** Live window-space chrome offsets (device px) when the worker frame carries
   *  effects chrome; undefined/0 in-process (the canvas rect IS the grid). */
  getChrome?: () => { pad: number; head: number }
}

export type AtermMouseInput = {
  dispose: () => void
}

// X10 button codes the engine encoders expect (left=0, middle=1, right=2).
const BUTTON_LEFT = 0
const BUTTON_MIDDLE = 1
const BUTTON_RIGHT = 2
// Motion with no button held is reported as button code 3 (engine adds bit 32).
const BUTTON_NONE = 3

// Modifier masks (aterm_types::mouse): Shift=4, Alt=8, Ctrl=16. Shift is the
// user's selection override and is NEVER folded into a forwarded report, so it
// is intentionally absent here — see aterm-mouse-forward-gate.
const ALT_MASK = 8
const CTRL_MASK = 16

/** True when this mouse event should be FORWARDED to the app as a mouse report
 *  rather than driving selection/scroll/link. Forwarding is on only when the TUI
 *  enabled mouse tracking AND the user is not holding Shift (Shift = override →
 *  fall through to selection/scroll exactly like a non-tracking terminal). Shared
 *  guard so selection/scroll/link defer to the same decision the forwarder uses. */
export function shouldForwardMouse(term: AtermTerminal, event: { shiftKey: boolean }): boolean {
  return term.is_mouse_tracking && !event.shiftKey
}

// Map a pointer position to a 0-based (col, row) on-screen cell. Identical
// mapping to selection/link input (clientX/Y minus the canvas rect, scaled to
// device pixels) so a forwarded report points at the same cell the user clicked.
// The engine encoders add the protocol's +1, so 0-based is correct here.
function pointToCell(event: MouseEvent, deps: AtermMouseDeps): { col: number; row: number } {
  const rect = deps.canvas.getBoundingClientRect()
  // Effects chrome shifts the canvas rect up-left of the grid (negative margins);
  // subtract the grid's in-frame offset so a report points at the clicked cell.
  const chrome = deps.getChrome?.() ?? { pad: 0, head: 0 }
  const deviceX = (event.clientX - rect.left) * deps.metrics.dpr - chrome.pad
  const deviceY = (event.clientY - rect.top) * deps.metrics.dpr - chrome.pad - chrome.head
  const col = Math.max(0, Math.floor(deviceX / deps.metrics.cellWidth))
  const row = Math.max(0, Math.floor(deviceY / deps.metrics.cellHeight))
  return { col, row }
}

// Build the modifier byte for a report. Shift is deliberately excluded: a
// Shift-held event never reaches the forwarder (shouldForwardMouse gates it),
// so only Alt/Ctrl are reported, matching xterm.
function modsByte(event: MouseEvent): number {
  let mods = 0
  if (event.altKey) {
    mods |= ALT_MASK
  }
  if (event.ctrlKey || event.metaKey) {
    mods |= CTRL_MASK
  }
  return mods
}

// Map a DOM mouse button (0=left,1=middle,2=right) to the engine's X10 code.
function buttonCode(domButton: number): number {
  if (domButton === 1) {
    return BUTTON_MIDDLE
  }
  if (domButton === 2) {
    return BUTTON_RIGHT
  }
  return BUTTON_LEFT
}

/** Forward canvas mouse events to the PTY as mouse reports when a TUI has
 *  enabled tracking. Listens in the CAPTURE phase so the report is sent (and the
 *  event neutralized for selection/scroll/link via the shared guard) before the
 *  bubble-phase selection/scroll/link handlers run; those handlers also call
 *  shouldForwardMouse and bail, so the gate is enforced on both sides. */
export function attachAtermMouseInput(deps: AtermMouseDeps): AtermMouseInput {
  const { canvas, term, metrics, getRows, inputSink, isDisposed } = deps
  // The button currently held during a drag (for 1002 motion); -1 = none.
  let heldButton = -1
  // Fractional wheel lines carried between events (trackpad sub-line deltas).
  let wheelRemainder = 0

  const send = (bytes: Uint8Array | undefined): void => {
    if (bytes && bytes.length > 0) {
      // Latin-1 round-trip: each report byte maps 1:1 to a char code, and the
      // input seam re-encodes with TextEncoder — keep bytes intact (no UTF-8
      // widening) so SGR/X10 reports reach the PTY verbatim.
      inputSink(String.fromCharCode(...bytes))
    }
  }

  const onMouseDown = (event: MouseEvent): void => {
    if (isDisposed() || !shouldForwardMouse(term, event)) {
      return
    }
    const { col, row } = pointToCell(event, deps)
    const button = buttonCode(event.button)
    heldButton = button
    send(term.encode_mouse_press(col, row, button, modsByte(event)))
    // Consume: do NOT start a text selection for a forwarded press.
    event.preventDefault()
    event.stopPropagation()
  }

  const onMouseUp = (event: MouseEvent): void => {
    if (isDisposed() || !shouldForwardMouse(term, event)) {
      heldButton = -1
      return
    }
    const { col, row } = pointToCell(event, deps)
    const button = buttonCode(event.button)
    heldButton = -1
    send(term.encode_mouse_release(col, row, button, modsByte(event)))
    event.preventDefault()
    event.stopPropagation()
  }

  const onMouseMove = (event: MouseEvent): void => {
    if (isDisposed() || !shouldForwardMouse(term, event)) {
      return
    }
    // 1000 (Normal) reports no motion at all; only 1002 (drag) / 1003 (any).
    if (!term.mouse_wants_motion) {
      return
    }
    // 1002 reports motion ONLY while a button is held; 1003 reports it always.
    const button = heldButton >= 0 ? heldButton : BUTTON_NONE
    if (button === BUTTON_NONE && !term.mouse_wants_any_motion) {
      return
    }
    const { col, row } = pointToCell(event, deps)
    const bytes = term.encode_mouse_motion(col, row, button, modsByte(event))
    if (bytes && bytes.length > 0) {
      send(bytes)
    }
    // We already decided (via the tracking-mode flags above) that this motion is a
    // forwarded report, so consume it regardless of whether bytes came back
    // synchronously — the single-engine worker encodes off-thread + sends the report
    // through the reply channel, returning nothing here.
    event.preventDefault()
    event.stopPropagation()
  }

  const onWheel = (event: WheelEvent): void => {
    if (isDisposed() || !shouldForwardMouse(term, event)) {
      return
    }
    // Same delta→lines accumulation as the scrollback path, so trackpad pixel
    // deltas produce line-paced reports instead of one report per DOM event.
    // Options are read live per event: the options bag mutates on settings change.
    const result = accumulateWheelLines({
      deltaY: event.deltaY,
      deltaMode: event.deltaMode,
      dpr: metrics.dpr,
      cellHeight: metrics.cellHeight,
      rows: getRows(),
      sensitivity: resolveTuiWheelMultiplier(event, deps.getTuiScrollMultiplier?.() ?? 1),
      remainder: wheelRemainder
    })
    wheelRemainder = result.remainder
    if (result.lines !== 0) {
      const { col, row } = pointToCell(event, deps)
      // Wheel-up (negative lines) reveals "up"; one report per accumulated line.
      const up = result.lines < 0
      for (let i = Math.abs(result.lines); i > 0; i--) {
        const bytes = term.encode_mouse_wheel(col, row, up, modsByte(event))
        if (bytes && bytes.length > 0) {
          send(bytes)
        }
      }
    }
    // Mouse tracking is on (gated above), so the wheel is a report to the app — consume
    // it so it doesn't also scroll scrollback. The worker encodes off-thread + sends
    // the report via the reply channel, returning nothing here.
    event.preventDefault()
    event.stopPropagation()
  }

  // Capture phase so the forwarder runs before selection/scroll/link.
  canvas.addEventListener('mousedown', onMouseDown, { capture: true })
  // mouseup/mousemove on window (capture) so a drag tracked off-canvas still
  // reports, mirroring selection-input's window-level mouseup.
  window.addEventListener('mouseup', onMouseUp, { capture: true })
  window.addEventListener('mousemove', onMouseMove, { capture: true })
  canvas.addEventListener('wheel', onWheel, { capture: true, passive: false })

  return {
    dispose: () => {
      canvas.removeEventListener('mousedown', onMouseDown, { capture: true })
      window.removeEventListener('mouseup', onMouseUp, { capture: true })
      window.removeEventListener('mousemove', onMouseMove, { capture: true })
      canvas.removeEventListener('wheel', onWheel, { capture: true })
    }
  }
}
