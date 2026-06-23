import type { AtermTerminal } from './aterm_wasm.js'

/** Sends encoded PTY bytes (mouse reports) to the child. Same seam selection
 *  copy / key encoding use — the controller threads pane.terminal.input here. */
export type AtermMouseInputSink = (data: string) => void

export type AtermMouseDeps = {
  canvas: HTMLCanvasElement
  term: AtermTerminal
  dpr: number
  cellWidth: number
  cellHeight: number
  inputSink: AtermMouseInputSink
  isDisposed: () => boolean
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
  const deviceX = (event.clientX - rect.left) * deps.dpr
  const deviceY = (event.clientY - rect.top) * deps.dpr
  const col = Math.max(0, Math.floor(deviceX / deps.cellWidth))
  const row = Math.max(0, Math.floor(deviceY / deps.cellHeight))
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
  const { canvas, term, inputSink, isDisposed } = deps
  // The button currently held during a drag (for 1002 motion); -1 = none.
  let heldButton = -1

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
      event.preventDefault()
      event.stopPropagation()
    }
  }

  const onWheel = (event: WheelEvent): void => {
    if (isDisposed() || !shouldForwardMouse(term, event)) {
      return
    }
    const { col, row } = pointToCell(event, deps)
    // Wheel-up (negative deltaY) reveals "up"; forward each notch as a wheel
    // report instead of scrolling scrollback.
    const up = event.deltaY < 0
    const bytes = term.encode_mouse_wheel(col, row, up, modsByte(event))
    if (bytes && bytes.length > 0) {
      send(bytes)
      event.preventDefault()
      event.stopPropagation()
    }
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
