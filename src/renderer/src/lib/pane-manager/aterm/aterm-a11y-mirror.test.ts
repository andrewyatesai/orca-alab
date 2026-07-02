/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAtermA11yMirror } from './aterm-a11y-mirror'

type FakeTerm = {
  row_text: (row: number) => string | undefined
  display_offset: number
  display_origin_absolute: number
}

function makeTerm(rows: string[], displayOffset = 0): FakeTerm {
  return {
    row_text: (r: number) => rows[r],
    display_offset: displayOffset,
    display_origin_absolute: 0
  }
}

describe('createAtermA11yMirror', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('appends visible main-screen output into the live region after the debounce', () => {
    const liveRegion = document.createElement('div')
    const term = makeTerm(['$ echo hi', 'hi', ''])
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 3,
      getCols: () => 80,
      isAltScreen: () => false,
      isDisposed: () => false
    })

    mirror.schedule()
    expect(liveRegion.textContent).toBe('') // debounced; nothing yet
    vi.advanceTimersByTime(250)
    // Trailing blank rows trimmed; each line is its own appended node.
    expect(liveRegion.childElementCount).toBe(2)
    expect(liveRegion.textContent).toContain('$ echo hi')
    expect(liveRegion.textContent).toContain('hi')
  })

  it('appends ONLY the new tail as output scrolls (no re-announce of old lines)', () => {
    const liveRegion = document.createElement('div')
    let rows = ['line 1', 'line 2', 'line 3']
    const term: FakeTerm = {
      row_text: (r) => rows[r],
      display_offset: 0,
      display_origin_absolute: 0
    }
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 3,
      getCols: () => 80,
      isAltScreen: () => false,
      isDisposed: () => false
    })

    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(liveRegion.childElementCount).toBe(3)

    // Output scrolled up by one: the window is now lines 2..4; only "line 4" is new.
    rows = ['line 2', 'line 3', 'line 4']
    term.display_origin_absolute = 1
    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(liveRegion.childElementCount).toBe(4) // appended exactly one
    expect(liveRegion.lastChild?.textContent).toBe('line 4')
  })

  it('updates an edited already-logged row IN PLACE (no whole-window re-append)', () => {
    const liveRegion = document.createElement('div')
    // The duplication bug: a logged prompt row is later edited in place into the
    // command echo; a text-overlap diff finds no overlap and re-appends the whole
    // window, duplicating (and potentially reordering) announced history.
    let rows = ['out A', '$']
    const term: FakeTerm = {
      row_text: (r) => rows[r],
      display_offset: 0,
      display_origin_absolute: 0
    }
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 4,
      getCols: () => 80,
      isAltScreen: () => false,
      isDisposed: () => false
    })

    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(liveRegion.childElementCount).toBe(2)

    // The prompt row is edited in place; new output appends below it. No scroll.
    rows = ['out A', '$ echo B', 'B', '$']
    mirror.schedule()
    vi.advanceTimersByTime(250)
    const texts = Array.from(liveRegion.children).map((c) => c.textContent)
    expect(texts).toEqual(['out A', '$ echo B', 'B', '$'])
  })

  it('re-anchors across a resize/rewrap: no duplicate window, new output still appends', () => {
    const liveRegion = document.createElement('div')
    // Seeded on a narrow grid (heavy wrapping, inflated origin) — like the initial
    // MIN-grid pane before the real reflow. A resize renumbers absolute lines
    // (origin can move BACKWARD), so a stale anchor would drop/corrupt rows.
    let rows = ['$ prompt']
    let cols = 20
    const term: FakeTerm = {
      row_text: (r) => rows[r],
      display_offset: 0,
      display_origin_absolute: 3
    }
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 4,
      getCols: () => cols,
      isAltScreen: () => false,
      isDisposed: () => false
    })

    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(liveRegion.childElementCount).toBe(1)

    // The reflow unwraps: origin drops back to 0 on a wider grid. The visible window
    // is already-announced content in its new wrap — do NOT re-append it.
    cols = 80
    term.display_origin_absolute = 0
    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(liveRegion.childElementCount).toBe(1)

    // Post-resize output appends normally from the re-seeded anchor.
    rows = ['$ prompt', 'fresh out']
    mirror.schedule()
    vi.advanceTimersByTime(250)
    const texts = Array.from(liveRegion.children).map((c) => c.textContent)
    expect(texts).toEqual(['$ prompt', 'fresh out'])
  })

  it('does not append when the screen is unchanged', () => {
    const liveRegion = document.createElement('div')
    const term = makeTerm(['same'])
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 1,
      getCols: () => 80,
      isAltScreen: () => false,
      isDisposed: () => false
    })

    mirror.schedule()
    vi.advanceTimersByTime(250)
    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(liveRegion.childElementCount).toBe(1) // appended once, not twice
  })

  it('does not append while the viewport is scrolled back (review mode)', () => {
    const liveRegion = document.createElement('div')
    const term = makeTerm(['history line'], 5) // display_offset > 0
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 1,
      getCols: () => 80,
      isAltScreen: () => false,
      isDisposed: () => false
    })

    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(liveRegion.childElementCount).toBe(0)
  })

  it('mirrors the visible grid verbatim on the alternate screen (TUI)', () => {
    const liveRegion = document.createElement('div')
    const term = makeTerm(['TUI top', 'TUI bottom'])
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 2,
      getCols: () => 80,
      isAltScreen: () => true,
      isDisposed: () => false
    })

    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(liveRegion.textContent).toBe('TUI top\nTUI bottom')
  })

  it('skips the refresh when disposed before the timer fires', () => {
    const liveRegion = document.createElement('div')
    let disposed = false
    const mirror = createAtermA11yMirror({
      liveRegion,
      term: makeTerm(['content']),
      getRows: () => 1,
      getCols: () => 80,
      isAltScreen: () => false,
      isDisposed: () => disposed
    })

    mirror.schedule()
    disposed = true
    vi.advanceTimersByTime(250)
    expect(liveRegion.textContent).toBe('')
  })

  it('dispose cancels a pending refresh', () => {
    const liveRegion = document.createElement('div')
    const mirror = createAtermA11yMirror({
      liveRegion,
      term: makeTerm(['content']),
      getRows: () => 1,
      getCols: () => 80,
      isAltScreen: () => false,
      isDisposed: () => false
    })

    mirror.schedule()
    mirror.dispose()
    vi.advanceTimersByTime(250)
    expect(liveRegion.textContent).toBe('')
  })
})
