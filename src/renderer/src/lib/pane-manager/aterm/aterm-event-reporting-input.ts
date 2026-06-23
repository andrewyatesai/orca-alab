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
  const { canvas, textarea, term, dpr, cellWidth, cellHeight, inputSink, isDisposed } = deps

  let lastMouseReport: string | null = null
  const mouseInput = attachAtermMouseInput({
    canvas,
    term,
    dpr,
    cellWidth,
    cellHeight,
    isDisposed,
    inputSink: (data) => {
      // e2e hook: record the last forwarded report so a test can prove a mouse
      // event reached the PTY without depending on shell echo under a hidden
      // window. Production cost is one field assignment.
      lastMouseReport = data
      inputSink(data)
    }
  })

  const focusInput = attachAtermFocusInput({ textarea, term, inputSink, isDisposed })

  return {
    lastMouseReport: () => lastMouseReport,
    dispose: () => {
      mouseInput.dispose()
      focusInput.dispose()
    }
  }
}
