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
  /** The engine: visible row text + the display offset (to skip appending while the
   *  viewport is scrolled back — that content is already in the log). */
  term: Pick<AtermTerminal, 'row_text' | 'display_offset'>
  /** Current visible row count (the wiring tracks this; the engine has no getter). */
  getRows: () => number
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
  while (lines.length > 0 && lines[lines.length - 1] === '') {
    lines.pop()
  }
  return lines
}

/** The largest k such that the TAIL of `log` equals the HEAD of `next` — the
 *  overlap, so only genuinely new lines (the suffix of `next` past it) are appended
 *  when output scrolls up or the bottom line is edited. */
function overlapLen(log: string[], next: string[]): number {
  const max = Math.min(log.length, next.length)
  for (let k = max; k > 0; k--) {
    let match = true
    for (let i = 0; i < k; i++) {
      if (log[log.length - k + i] !== next[i]) {
        match = false
        break
      }
    }
    if (match) {
      return k
    }
  }
  return 0
}

export function createAtermA11yMirror(deps: AtermA11yMirrorDeps): AtermA11yMirror {
  const { liveRegion, term, getRows, isAltScreen, isDisposed } = deps
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  // The accumulated announced output (main-screen log), for the overlap diff.
  let log: string[] = []
  let altMode = false

  const clearRegion = (): void => {
    while (liveRegion.firstChild) {
      liveRegion.removeChild(liveRegion.firstChild)
    }
    log = []
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
    // Append only the new tail so aria-live announces just the new output, and the
    // accumulated children give screen-reader scrollback review.
    const fresh = visible.slice(overlapLen(log, visible))
    if (fresh.length === 0) {
      return
    }
    for (const line of fresh) {
      log.push(line)
      const node = document.createElement('div')
      node.textContent = line
      liveRegion.appendChild(node)
    }
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
