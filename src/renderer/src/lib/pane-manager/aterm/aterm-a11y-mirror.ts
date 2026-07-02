import type { AtermTerminal } from './aterm_wasm.js'

/** Mirrors the aterm engine's output into an off-screen ARIA live region so screen
 *  readers can announce + review terminal output — the <canvas> the renderer paints
 *  to is opaque to assistive tech, and xterm's AccessibilityManager is unreachable
 *  under aterm (terminal.open() is never called). It reads the ENGINE (row_text), so
 *  it works on both the CPU and GPU draw paths.
 *
 *  On the MAIN screen it is an APPEND-ONLY log: each refresh appends only the lines
 *  that are genuinely new (output that scrolled up since the last refresh), as
 *  discrete child nodes. That (a) lets aria-live announce only the new output
 *  instead of re-announcing the whole screen every frame, and (b) accumulates the
 *  output history so a screen-reader review cursor can read back through SCROLLBACK
 *  — the gap vs the old visible-only mirror. On the ALTERNATE screen (vim/htop/less,
 *  which is not a streaming log) it mirrors the visible grid verbatim instead. */
export type AtermA11yMirror = {
  /** Schedule a (debounced) refresh of the live region from the current grid. */
  schedule: () => void
  /** Cancel any pending refresh (call on teardown). */
  dispose: () => void
}

export type AtermA11yMirrorDeps = {
  /** The off-screen role="log" / aria-live element to write the grid text into. */
  liveRegion: HTMLElement
  /** The engine: visible row text, the display offset (to skip appending while the
   *  viewport is scrolled back — that content is already in the log), and the absolute
   *  line of visible row 0 (the log's append anchor). */
  term: Pick<AtermTerminal, 'row_text' | 'display_offset' | 'display_origin_absolute'>
  /** Current visible row count (the wiring tracks this; the engine has no getter). */
  getRows: () => number
  /** Current column count — a cols/rows change rewraps the buffer and renumbers
   *  absolute lines, so the append anchor must re-seed. */
  getCols: () => number
  /** True on the alternate screen (TUI) — mirror the visible grid, don't append. */
  isAltScreen: () => boolean
  isDisposed: () => boolean
}

// ~250ms: slow enough to coalesce a burst of PTY chunks into one announcement,
// fast enough that output reaches a screen reader without a perceptible lag.
const REFRESH_DEBOUNCE_MS = 250

// Cap the accumulated log so a long session keeps the off-screen DOM bounded;
// ~2000 lines is ample scrollback for screen-reader review.
const MAX_LOG_LINES = 2000

/** Visible rows as text, trailing blank rows trimmed. */
function readVisibleLines(term: Pick<AtermTerminal, 'row_text'>, rows: number): string[] {
  const lines: string[] = []
  for (let r = 0; r < rows; r++) {
    lines.push((term.row_text(r) ?? '').replace(/\s+$/, ''))
  }
  while (lines.length > 0 && lines.at(-1) === '') {
    lines.pop()
  }
  return lines
}

export function createAtermA11yMirror(deps: AtermA11yMirrorDeps): AtermA11yMirror {
  const { liveRegion, term, getRows, getCols, isAltScreen, isDisposed } = deps
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  // The accumulated announced output (main-screen log). Its entries map 1:1 onto
  // liveRegion's child <div>s (both are appended together and trimmed together).
  let log: string[] = []
  // ABSOLUTE line (display_origin_absolute + row) of the log's last entry: appends key
  // off WHERE a row lives, not its text. A text-overlap diff mis-fires when an
  // already-logged row is edited in place (prompt → command echo, clear-screen redraw)
  // and re-appends the whole visible window — duplicated, order-breaking history.
  let lastAbs = -1
  // Grid dims + last seen origin: a resize REWRAPS the buffer (renumbering absolute
  // lines, origin can even move backward), so the anchor is meaningless across one.
  let lastCols = -1
  let lastRows = -1
  let lastOrigin = -1
  let altMode = false

  const clearRegion = (): void => {
    while (liveRegion.firstChild) {
      liveRegion.removeChild(liveRegion.firstChild)
    }
    log = []
    lastAbs = -1
  }

  const refresh = (): void => {
    timeoutId = null
    if (isDisposed()) {
      return
    }
    const visible = readVisibleLines(term, getRows())

    if (isAltScreen()) {
      // TUI/alt screen: not a streaming log — mirror the visible grid verbatim.
      if (!altMode) {
        clearRegion()
        altMode = true
      }
      const text = visible.join('\n')
      if (liveRegion.textContent !== text) {
        liveRegion.textContent = text
      }
      return
    }

    // Returned to the main screen: reset the alt snapshot, start logging fresh.
    if (altMode) {
      clearRegion()
      altMode = false
    }
    // While scrolled back the user is reviewing existing history — don't append the
    // scrolled viewport (it would duplicate older lines onto the end of the log).
    if (term.display_offset !== 0) {
      return
    }
    if (visible.length === 0) {
      return
    }
    const origin = term.display_origin_absolute
    const cols = getCols()
    const rows = getRows()
    const rewrapped = cols !== lastCols || rows !== lastRows || origin < lastOrigin
    lastCols = cols
    lastRows = rows
    lastOrigin = origin
    if (rewrapped && lastAbs >= 0) {
      // Re-anchor at the current bottom WITHOUT appending: the visible window is
      // previously-announced content in its new wrap — re-appending would duplicate
      // (and reorder) the log; edits against the stale anchor would corrupt it.
      lastAbs = origin + visible.length - 1
      return
    }
    // Still-visible rows already in the log: an edit rewrites that row's existing
    // node (announced via aria-atomic=false) instead of growing the log.
    const alreadyLogged = Math.max(0, lastAbs - origin + 1)
    const editable = Math.min(alreadyLogged, visible.length)
    const logStart = log.length - alreadyLogged
    for (let i = 0; i < editable; i++) {
      const at = logStart + i
      // at < 0: the row's node was trimmed off by the log cap — nothing to update.
      if (at < 0 || log[at] === visible[i]) {
        continue
      }
      log[at] = visible[i]
      const node = liveRegion.children[at]
      if (node) {
        node.textContent = visible[i]
      }
    }
    // Append only rows past the anchor so aria-live announces just the new output,
    // and the accumulated children give screen-reader scrollback review.
    for (let i = editable; i < visible.length; i++) {
      log.push(visible[i])
      const node = document.createElement('div')
      node.textContent = visible[i]
      liveRegion.appendChild(node)
    }
    lastAbs = Math.max(lastAbs, origin + visible.length - 1)
    if (log.length > MAX_LOG_LINES) {
      log = log.slice(-MAX_LOG_LINES)
      while (liveRegion.childElementCount > MAX_LOG_LINES && liveRegion.firstChild) {
        liveRegion.removeChild(liveRegion.firstChild)
      }
    }
  }

  return {
    schedule: () => {
      if (isDisposed() || timeoutId !== null) {
        return
      }
      timeoutId = setTimeout(refresh, REFRESH_DEBOUNCE_MS)
    },
    dispose: () => {
      if (timeoutId !== null) {
        clearTimeout(timeoutId)
        timeoutId = null
      }
    }
  }
}
