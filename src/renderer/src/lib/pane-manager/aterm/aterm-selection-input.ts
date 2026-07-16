import type { AtermTerminal } from './aterm_wasm.js'
import { shouldForwardMouse } from './aterm-mouse-input'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'

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
  /** Whether drag / double-click / triple-click should AUTO-copy the selection
   *  (xterm's copyOnSelect; orca's terminalClipboardOnSelect, default false). When
   *  false, selecting must NOT touch the clipboard — only explicit Cmd/Ctrl+C does.
   *  Read live so a settings toggle applies without recreating the pane. */
  getCopyOnSelect?: () => boolean
  /** Fired after each mouse-driven selection mutation so the facade can emit
   *  onSelectionChange without waiting for PTY output (Linux PRIMARY on idle
   *  shells). The facade dedupes by range, so worker-path snapshot lag only
   *  delays — never doubles — the emit. */
  onSelectionChanged?: () => void
  /** Live window-space chrome offsets (device px) when the worker frame carries
   *  effects chrome; undefined/0 in-process (the canvas rect IS the grid). */
  getChrome?: () => { pad: number; head: number }
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
function pointToCell(event: MouseEvent, deps: AtermSelectionDeps): { col: number; row: number } {
  const rect = deps.canvas.getBoundingClientRect()
  // Effects chrome shifts the canvas rect up-left of the grid (negative margins);
  // subtract the grid's in-frame offset so cell math stays grid-relative.
  const chrome = deps.getChrome?.() ?? { pad: 0, head: 0 }
  const deviceX = (event.clientX - rect.left) * deps.dpr - chrome.pad
  const deviceY = (event.clientY - rect.top) * deps.dpr - chrome.pad - chrome.head
  const col = Math.max(0, Math.floor(deviceX / deps.cellWidth))
  const row = Math.max(0, Math.floor(deviceY / deps.cellHeight))
  return { col, row }
}

/** Wire mouse drag → aterm text selection on the canvas, and expose a copy
 *  action (shared by mouseup and Cmd/Ctrl+C). The CPU renderer paints the
 *  highlight from the grid's selection, so we only redraw after each change. */
export function attachAtermSelectionInput(deps: AtermSelectionDeps): AtermSelectionInput {
  const { canvas, term, redraw, isDisposed, onCopy, getCopyOnSelect, onSelectionChanged } = deps
  let dragging = false
  const copyOnSelect = (): boolean => getCopyOnSelect?.() ?? false

  // Worker-backed term: selection_text()/selection_word()/selection_line() lag the
  // posted selection (the snapshot updates a frame later), so copy-on-select reads the
  // fresh text via the async query. In-process exposes no such method → sync read
  // (byte-identical). Cmd/Ctrl+C keeps the sync copySelection() (it fires after settle).
  const asyncSelectionText = (term as AtermTerminal & Partial<AtermWorkerAsyncFacade>)
    .selectionTextAsync

  const copySelection = (): boolean => {
    const text = term.selection_text()
    if (text === undefined || text.length === 0) {
      return false
    }
    onCopy(text)
    return true
  }

  // Copy after a posted selection change: await the worker's fresh text when available,
  // else read it synchronously. Guards disposal + empty so it never clobbers the
  // clipboard with a cleared/stale selection.
  const copyAfterSelectionChange = (): void => {
    if (asyncSelectionText) {
      void asyncSelectionText().then((text) => {
        if (!isDisposed() && text.length > 0) {
          onCopy(text)
        }
      })
      return
    }
    copySelection()
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
    const { col, row } = pointToCell(event, deps)
    // Double-click → word/URL (semantic) selection; triple-click → whole line.
    // The engine sets the grid selection (so the highlight paints on redraw) and
    // returns the selected text, which we copy to match drag-select's copy-on-up.
    // detail counts clicks in a burst, so a drag started this same press would
    // override these; we don't set `dragging`, so the click select stands.
    if (event.detail === 2 || event.detail === 3) {
      const selected =
        event.detail === 2 ? term.selection_word(row, col) : term.selection_line(row, col)
      redraw()
      onSelectionChanged?.()
      // Auto-copy only when copy-on-select is enabled (default off) — otherwise the
      // selection just highlights and Cmd/Ctrl+C copies it.
      if (!copyOnSelect()) {
        return
      }
      // Worker-backed selection_word/line can't return the text synchronously (they post;
      // the snapshot lags), so copy the fresh text via the async query. In-process returns
      // it directly — keep that exact path.
      if (asyncSelectionText) {
        copyAfterSelectionChange()
      } else if (selected !== undefined && selected.length > 0) {
        onCopy(selected)
      }
      return
    }
    // Fresh selection on every primary single click.
    term.selection_clear()
    term.selection_start(row, col)
    dragging = true
    redraw()
    onSelectionChanged?.()
  }

  const onMouseMove = (event: MouseEvent): void => {
    if (!dragging || isDisposed()) {
      return
    }
    const { col, row } = pointToCell(event, deps)
    term.selection_extend(row, col)
    redraw()
    onSelectionChanged?.()
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
    onSelectionChanged?.()
    // Drag-select auto-copies ONLY when copy-on-select is on (default off); without
    // this guard every drag clobbered the user's clipboard. Cmd/Ctrl+C still copies
    // unconditionally via copySelection() below.
    if (copyOnSelect()) {
      copyAfterSelectionChange()
    }
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
