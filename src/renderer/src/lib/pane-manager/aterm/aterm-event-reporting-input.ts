import type { AtermTerminal } from './aterm_wasm.js'
import { attachAtermMouseInput } from './aterm-mouse-input'
import { attachAtermFocusInput } from './aterm-focus-input'

/** Sends report bytes (mouse / focus) to the PTY — the controller's input seam. */
export type AtermReportSink = (data: string) => void

export type AtermEventReportingDeps = {
  canvas: HTMLCanvasElement
  /** Hidden helper textarea that owns keyboard focus (focus/blur = pane focus). */
  textarea: HTMLTextAreaElement
  term: AtermTerminal
  dpr: number
  cellWidth: number
  cellHeight: number
  inputSink: AtermReportSink
  isDisposed: () => boolean
}

export type AtermEventReportingInput = {
  /** e2e/test hook: the last mouse REPORT forwarded to the PTY, or null. */
  lastMouseReport: () => string | null
  /** Update the device-pixel ratio used for mouse-report hit-testing after the
   *  window moves to a different-DPI monitor (M2). */
  setDpr: (dpr: number) => void
  dispose: () => void
}

/** Wire the two "host event → PTY report" layers a TUI can ask for:
 *  - MOUSE reporting (DECSET 1000/1002/1003 + encoding): canvas mouse events are
 *    encoded and sent so vim/tmux/htop respond to the mouse (selection/scroll/
 *    link defer via the shared shouldForwardMouse gate; Shift = user override).
 *  - FOCUS reporting (DECSET 1004): the helper textarea's focus/blur sends
 *    CSI I / CSI O so apps track terminal focus.
 *  Bundled here so the controller stays under the line budget. */
export function attachAtermEventReportingInput(
  deps: AtermEventReportingDeps
): AtermEventReportingInput {
  const { canvas, textarea, term, cellWidth, cellHeight, inputSink, isDisposed } = deps

  let lastMouseReport: string | null = null
  // Mutable so a DPI change (setDpr) re-targets mouse-report hit-testing;
  // mouse-input reads this object's `dpr` live in pointToCell (M2).
  const mouseDeps = {
    canvas,
    term,
    dpr: deps.dpr,
    cellWidth,
    cellHeight,
    isDisposed,
    inputSink: (data: string) => {
      // e2e hook: record the last forwarded report so a test can prove a mouse
      // event reached the PTY without depending on shell echo under a hidden
      // window. Production cost is one field assignment.
      lastMouseReport = data
      inputSink(data)
    }
  }
  const mouseInput = attachAtermMouseInput(mouseDeps)

  const focusInput = attachAtermFocusInput({ textarea, term, inputSink, isDisposed })

  return {
    lastMouseReport: () => lastMouseReport,
    setDpr: (next: number) => {
      mouseDeps.dpr = next
    },
    dispose: () => {
      mouseInput.dispose()
      focusInput.dispose()
    }
  }
}
