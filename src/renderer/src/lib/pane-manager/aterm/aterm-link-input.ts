import type { AtermTerminal } from './aterm_wasm.js'
import { shouldForwardMouse } from './aterm-mouse-input'
import type { AtermHoveredLinkSpan } from './aterm-link-underline-overlay'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'

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
  /** The display-row cell span of the link currently under the pointer, or null.
   *  The draw paths read this each frame to paint the hover underline; it's
   *  cleared whenever the pointer leaves the link / a non-link cell / alt-screen. */
  hoveredSpan: () => AtermHoveredLinkSpan | null
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

// Two hovered link spans are equal when they cover the same cells; used to avoid
// redrawing the underline while the pointer moves within one link span.
function spansEqual(a: AtermHoveredLinkSpan | null, b: AtermHoveredLinkSpan | null): boolean {
  if (a === null || b === null) {
    return a === b
  }
  return a.row === b.row && a.startCol === b.startCol && a.endCol === b.endCol
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
  const { canvas, term, redraw, isDisposed, openUrl, getFileLinkOpener } = deps
  // Worker-backed term: link_at returns the lagging snapshot and the loader drives the
  // canvas cursor from the worker's hoverCursor each STATE. Detect the async capability
  // to resolve fresh hits on click + clear the worker hover, and stop fighting the
  // loader's cursor. In-process exposes neither → the synchronous path below is
  // byte-identical.
  const workerTerm = term as AtermTerminal & Partial<AtermWorkerAsyncFacade>
  const asyncLinkAt = workerTerm.linkAtAsync
  const clearWorkerHover = workerTerm.clearHover
  let moveScheduled = false
  let lastCol = -1
  let lastRow = -1
  let pendingEvent: MouseEvent | null = null
  // Tracked so dispose() can cancel a pending hover frame (cleared when it fires).
  let hoverRafId: number | null = null
  // The link span under the pointer (display-row cells); the draw paths read it to
  // paint the hover underline. Cleared whenever the cursor affordance is cleared,
  // and a redraw is requested only when it actually changes so the underline
  // appears/disappears without per-pixel repaints.
  let hovered: AtermHoveredLinkSpan | null = null

  // Drop the link affordance (pointer cursor + underline). Requests a redraw only
  // when an underline was actually showing, so a non-link move stays cheap.
  const clearCursor = (): void => {
    // Worker path: clear the worker's hover (→ '' next STATE; the loader applies the
    // cursor) instead of writing the canvas cursor here, which that STATE would
    // overwrite. In-process: clear the cursor directly (byte-identical).
    if (clearWorkerHover) {
      clearWorkerHover()
    } else {
      canvas.style.cursor = ''
    }
    if (hovered) {
      hovered = null
      redraw()
    }
  }

  // Throttle hover hit-testing to one rAF frame, and skip re-querying when the
  // pointer is still on the same cell (mousemove fires per pixel).
  const evaluateHover = (): void => {
    moveScheduled = false
    hoverRafId = null
    const event = pendingEvent
    pendingEvent = null
    if (!event || isDisposed()) {
      return
    }
    // On the alternate screen TUIs own the mouse; never show a link cursor.
    // Likewise when mouse tracking is on (no Shift): the app owns the pointer,
    // so don't show a link cursor — the forwarder is reporting motion to it.
    if (term.is_alt_screen || shouldForwardMouse(term, event)) {
      clearCursor()
      return
    }
    const { col, row } = pointToCell(event, deps)
    if (col === lastCol && row === lastRow) {
      return
    }
    lastCol = col
    lastRow = row
    // NOTE: the wasm signature is link_at(row, col) — match the .d.ts order. The call
    // still posts the hover position the worker needs to compute its underline + cursor.
    const hit = term.link_at(row, col)
    // In-process answers link_at synchronously → set the cursor here. Worker-backed:
    // link_at lags a frame and the loader drives the cursor from hoverCursor, so don't
    // overwrite it with a stale value.
    if (!asyncLinkAt) {
      canvas.style.cursor = hit ? 'pointer' : ''
    }
    // Track the hovered span so the draw paths underline it; redraw only when the
    // span actually changes (entering/leaving a link, or moving to a different
    // link span) — moving within the same link span is a no-op.
    const nextSpan: AtermHoveredLinkSpan | null = hit
      ? { row, startCol: hit.start_col, endCol: hit.end_col }
      : null
    if (!spansEqual(hovered, nextSpan)) {
      hovered = nextSpan
      redraw()
    }
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
    hoverRafId = requestAnimationFrame(evaluateHover)
  }

  // Open a resolved link hit (URL/OSC8 via openUrl, file-path via the late-bound opener).
  const openHit = (hit: { url: string; kind: number }, event: MouseEvent): void => {
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

  const onClick = (event: MouseEvent): void => {
    if (isDisposed() || event.button !== 0 || !isLinkActivation(event)) {
      return
    }
    // Mouse tracking on (no Shift) → the click is a report to the app, not a
    // link activation; defer just like the alternate-screen case.
    if (term.is_alt_screen || shouldForwardMouse(term, event)) {
      return
    }
    const { col, row } = pointToCell(event, deps)
    // Worker-backed term: the sync link_at snapshot lags, so resolve the real hit via the
    // async query channel; in-process answers synchronously.
    if (asyncLinkAt) {
      void asyncLinkAt(row, col).then((hit) => {
        if (!isDisposed() && hit) {
          openHit(hit, event)
        }
      })
      return
    }
    const hit = term.link_at(row, col)
    if (!hit) {
      return
    }
    openHit(hit, event)
  }

  canvas.addEventListener('mousemove', onMouseMove)
  canvas.addEventListener('click', onClick)

  return {
    hoveredSpan: () => hovered,
    dispose: () => {
      // Cancel a queued hover frame so evaluateHover can't run after teardown.
      if (hoverRafId !== null) {
        cancelAnimationFrame(hoverRafId)
        hoverRafId = null
      }
      canvas.removeEventListener('mousemove', onMouseMove)
      canvas.removeEventListener('click', onClick)
      clearCursor()
    }
  }
}
