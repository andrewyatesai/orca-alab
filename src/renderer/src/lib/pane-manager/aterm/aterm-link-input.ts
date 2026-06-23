import type { AtermTerminal } from './aterm_wasm.js'

/** Opens a detected link target; the controller threads orca's URL opener here
 *  (forceSystemBrowser mirrors xterm's Shift+modifier "open in system browser"
 *  escape hatch). */
export type AtermLinkOpener = (url: string, opts: { forceSystemBrowser: boolean }) => void

/** Opens a detected file-path link (kind 2). `rawPathText` is the matched span
 *  exactly as it appeared on the row; the closure resolves it against the pane's
 *  cwd/runtime and opens it. `openWithSystemDefault` mirrors xterm's Shift hatch. */
export type AtermFileLinkOpener = (rawPathText: string, openWithSystemDefault: boolean) => void

export type AtermLinkDeps = {
  canvas: HTMLCanvasElement
  term: AtermTerminal
  dpr: number
  cellWidth: number
  cellHeight: number
  redraw: () => void
  isDisposed: () => boolean
  openUrl: AtermLinkOpener
  /** Latest file-path opener (kind 2), late-bound by the controller. Null until
   *  the pane's cwd/runtime context is threaded in; then kind-2 clicks open. */
  getFileLinkOpener: () => AtermFileLinkOpener | null
}

export type AtermLinkInput = {
  dispose: () => void
}

// Link kinds from the wasm engine: 0=osc8, 1=url, 2=file_path, 3=other.
const LINK_KIND_OSC8 = 0
const LINK_KIND_URL = 1
const LINK_KIND_FILE_PATH = 2

// Map a pointer position to a (col, display-row) grid cell. Identical mapping to
// aterm-selection-input.ts: clientX/Y minus the canvas rect (not offsetX/Y) so
// synthetic e2e events and real events agree, scaled to device pixels; the row
// is already display-offset-inclusive.
function pointToCell(event: MouseEvent, deps: AtermLinkDeps): { col: number; row: number } {
  const rect = deps.canvas.getBoundingClientRect()
  const deviceX = (event.clientX - rect.left) * deps.dpr
  const deviceY = (event.clientY - rect.top) * deps.dpr
  const col = Math.max(0, Math.floor(deviceX / deps.cellWidth))
  const row = Math.max(0, Math.floor(deviceY / deps.cellHeight))
  return { col, row }
}

// Platform link-activation modifier: Cmd on macOS, Ctrl elsewhere. Mirrors
// terminal-link-handlers.isTerminalLinkActivation so the aterm path matches the
// default terminal's "modifier+click opens the link" convention.
function isLinkActivation(event: MouseEvent): boolean {
  const isMac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac')
  return isMac ? event.metaKey : event.ctrlKey
}

/** Wire hover + modifier-click link activation on the aterm canvas. Mirrors
 *  attachAtermSelectionInput's structure; the wasm engine does the link
 *  detection via link_at, and we only paint a pointer cursor + open URLs. */
export function attachAtermLinkInput(deps: AtermLinkDeps): AtermLinkInput {
  const { canvas, term, isDisposed, openUrl, getFileLinkOpener } = deps
  let moveScheduled = false
  let lastCol = -1
  let lastRow = -1
  let pendingEvent: MouseEvent | null = null

  const clearCursor = (): void => {
    canvas.style.cursor = ''
  }

  // Throttle hover hit-testing to one rAF frame, and skip re-querying when the
  // pointer is still on the same cell (mousemove fires per pixel).
  const evaluateHover = (): void => {
    moveScheduled = false
    const event = pendingEvent
    pendingEvent = null
    if (!event || isDisposed()) {
      return
    }
    // On the alternate screen TUIs own the mouse; never show a link cursor.
    if (term.is_alt_screen) {
      clearCursor()
      return
    }
    const { col, row } = pointToCell(event, deps)
    if (col === lastCol && row === lastRow) {
      return
    }
    lastCol = col
    lastRow = row
    // NOTE: the wasm signature is link_at(row, col) — match the .d.ts order.
    const hit = term.link_at(row, col)
    canvas.style.cursor = hit ? 'pointer' : ''
  }

  const onMouseMove = (event: MouseEvent): void => {
    if (isDisposed()) {
      return
    }
    pendingEvent = event
    if (moveScheduled) {
      return
    }
    moveScheduled = true
    requestAnimationFrame(evaluateHover)
  }

  const onClick = (event: MouseEvent): void => {
    if (isDisposed() || event.button !== 0 || !isLinkActivation(event)) {
      return
    }
    if (term.is_alt_screen) {
      return
    }
    const { col, row } = pointToCell(event, deps)
    const hit = term.link_at(row, col)
    if (!hit) {
      return
    }
    if (hit.kind === LINK_KIND_OSC8 || hit.kind === LINK_KIND_URL) {
      event.preventDefault()
      openUrl(hit.url, { forceSystemBrowser: event.shiftKey })
      return
    }
    // File paths: defer to the late-bound opener (resolves cwd/runtime + opens).
    // Null until the pane's context is threaded in → no-op, never a crash.
    if (hit.kind === LINK_KIND_FILE_PATH) {
      const openFileLink = getFileLinkOpener()
      if (!openFileLink) {
        return
      }
      event.preventDefault()
      openFileLink(hit.url, event.shiftKey)
    }
  }

  canvas.addEventListener('mousemove', onMouseMove)
  canvas.addEventListener('click', onClick)

  return {
    dispose: () => {
      canvas.removeEventListener('mousemove', onMouseMove)
      canvas.removeEventListener('click', onClick)
      clearCursor()
    }
  }
}
