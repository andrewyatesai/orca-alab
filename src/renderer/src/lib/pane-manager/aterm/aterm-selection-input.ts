import type { AtermTerminal } from './aterm_wasm.js'
import { shouldForwardMouse } from './aterm-mouse-input'

export type AtermSelectionDeps = {
  canvas: HTMLCanvasElement
  term: AtermTerminal
  dpr: number
  cellWidth: number
  cellHeight: number
  redraw: () => void
  isDisposed: () => boolean
  /** Last copied text, surfaced for tests; production also writes the clipboard. */
  onCopy: (text: string) => void
}

export type AtermSelectionInput = {
  /** Copy the current selection if non-empty; returns true when something was
   *  copied (so Cmd/Ctrl+C can swallow the key instead of sending ^C). */
  copySelection: () => boolean
  dispose: () => void
}

// Map a pointer position to a (col, display-row) grid cell. Use clientX/Y minus
// the canvas rect (not offsetX/Y) so synthetic e2e events and real events agree;
// aterm's selection rows are display rows (already include the scrollback offset
// via display_offset), so the visible row index maps 1:1.
function pointToCell(
  event: MouseEvent,
  deps: AtermSelectionDeps
): { col: number; row: number } {
  const rect = deps.canvas.getBoundingClientRect()
  const deviceX = (event.clientX - rect.left) * deps.dpr
  const deviceY = (event.clientY - rect.top) * deps.dpr
  const col = Math.max(0, Math.floor(deviceX / deps.cellWidth))
  const row = Math.max(0, Math.floor(deviceY / deps.cellHeight))
  return { col, row }
}

/** Wire mouse drag → aterm text selection on the canvas, and expose a copy
 *  action (shared by mouseup and Cmd/Ctrl+C). The CPU renderer paints the
 *  highlight from the grid's selection, so we only redraw after each change. */
export function attachAtermSelectionInput(deps: AtermSelectionDeps): AtermSelectionInput {
  const { canvas, term, redraw, isDisposed, onCopy } = deps
  let dragging = false

  const copySelection = (): boolean => {
    const text = term.selection_text()
    if (text === undefined || text.length === 0) {
      return false
    }
    onCopy(text)
    return true
  }

  const onMouseDown = (event: MouseEvent): void => {
    if (isDisposed() || event.button !== 0) {
      return
    }
    // Defer to the mouse forwarder when a TUI has mouse tracking on (no Shift):
    // that press is a mouse report to the app, not a selection. Shift held →
    // shouldForwardMouse is false → selection runs (user override).
    if (shouldForwardMouse(term, event)) {
      return
    }
    // Fresh selection on every primary click.
    term.selection_clear()
    const { col, row } = pointToCell(event, deps)
    term.selection_start(row, col)
    dragging = true
    redraw()
  }

  const onMouseMove = (event: MouseEvent): void => {
    if (!dragging || isDisposed()) {
      return
    }
    const { col, row } = pointToCell(event, deps)
    term.selection_extend(row, col)
    redraw()
  }

  const onMouseUp = (): void => {
    if (!dragging) {
      return
    }
    dragging = false
    if (isDisposed()) {
      return
    }
    term.selection_finish()
    redraw()
    copySelection()
  }

  canvas.addEventListener('mousedown', onMouseDown)
  canvas.addEventListener('mousemove', onMouseMove)
  // mouseup on window so a drag that ends off-canvas still completes.
  window.addEventListener('mouseup', onMouseUp)

  return {
    copySelection,
    dispose: () => {
      canvas.removeEventListener('mousedown', onMouseDown)
      canvas.removeEventListener('mousemove', onMouseMove)
      window.removeEventListener('mouseup', onMouseUp)
    }
  }
}
