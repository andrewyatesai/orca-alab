import type { AtermTerminal } from './aterm_wasm.js'

/** Mirrors the aterm engine's visible grid text into an off-screen ARIA live
 *  region so screen readers can announce terminal output — the <canvas> the
 *  renderer paints to is opaque to assistive tech, and xterm's Accessibility
 *  Manager is unreachable under aterm (terminal.open() is never called). It reads
 *  the ENGINE (term.row_text), not the canvas, so it works on both the CPU and
 *  GPU draw paths. Updates are debounced and skipped when the text is unchanged
 *  to keep it cheap and avoid spamming the live region. */
export type AtermA11yMirror = {
  /** Schedule a (debounced) refresh of the live region from the current grid. */
  schedule: () => void
  /** Cancel any pending refresh (call on teardown). */
  dispose: () => void
}

export type AtermA11yMirrorDeps = {
  /** The off-screen role="log" / aria-live element to write the grid text into. */
  liveRegion: HTMLElement
  /** The engine to read the display-correct visible rows from. */
  term: Pick<AtermTerminal, 'row_text'>
  /** Current visible row count (the wiring tracks this; the engine has no getter). */
  getRows: () => number
  isDisposed: () => boolean
}

// ~250ms: slow enough to coalesce a burst of PTY chunks into one announcement,
// fast enough that output reaches a screen reader without a perceptible lag.
const REFRESH_DEBOUNCE_MS = 250

/** Read the visible grid as plain text, trimming trailing blank rows so the
 *  announced content is just what's on screen (not the empty tail of the grid). */
function readVisibleGridText(term: Pick<AtermTerminal, 'row_text'>, rows: number): string {
  const lines: string[] = []
  for (let r = 0; r < rows; r++) {
    lines.push((term.row_text(r) ?? '').replace(/\s+$/, ''))
  }
  while (lines.length > 0 && lines[lines.length - 1] === '') {
    lines.pop()
  }
  return lines.join('\n')
}

export function createAtermA11yMirror(deps: AtermA11yMirrorDeps): AtermA11yMirror {
  const { liveRegion, term, getRows, isDisposed } = deps
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  let lastText = ''

  const clearTimer = (): void => {
    if (timeoutId !== null) {
      clearTimeout(timeoutId)
      timeoutId = null
    }
  }

  const refresh = (): void => {
    timeoutId = null
    if (isDisposed()) {
      return
    }
    const text = readVisibleGridText(term, getRows())
    // Only write on real change so the live region isn't re-announced every frame
    // (a no-op textContent write can still re-trigger some screen readers).
    if (text === lastText) {
      return
    }
    lastText = text
    liveRegion.textContent = text
  }

  return {
    schedule: () => {
      if (isDisposed() || timeoutId !== null) {
        return
      }
      timeoutId = setTimeout(refresh, REFRESH_DEBOUNCE_MS)
    },
    dispose: clearTimer
  }
}
