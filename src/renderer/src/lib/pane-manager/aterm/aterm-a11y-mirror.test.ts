/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAtermA11yMirror } from './aterm-a11y-mirror'

type RowReader = { row_text: (row: number) => string | undefined }

function makeRows(rows: string[]): RowReader {
  return { row_text: (r: number) => rows[r] }
}

describe('createAtermA11yMirror', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('mirrors the visible grid text into the live region after the debounce', () => {
    const liveRegion = document.createElement('div')
    const term = makeRows(['$ echo hi', 'hi', ''])
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 3,
      isDisposed: () => false
    })

    mirror.schedule()
    expect(liveRegion.textContent).toBe('') // debounced; nothing yet
    vi.advanceTimersByTime(250)
    // Trailing blank rows trimmed; visible content preserved.
    expect(liveRegion.textContent).toBe('$ echo hi\nhi')
  })

  it('coalesces a burst of schedules into a single write', () => {
    const liveRegion = document.createElement('div')
    const term = makeRows(['line one', 'line two'])
    const setSpy = vi.spyOn(liveRegion, 'textContent', 'set')
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 2,
      isDisposed: () => false
    })

    mirror.schedule()
    mirror.schedule()
    mirror.schedule()
    vi.advanceTimersByTime(250)
    expect(setSpy).toHaveBeenCalledTimes(1)
    expect(liveRegion.textContent).toBe('line one\nline two')
  })

  it('does not rewrite the live region when the text is unchanged', () => {
    const liveRegion = document.createElement('div')
    const term = makeRows(['same'])
    const setSpy = vi.spyOn(liveRegion, 'textContent', 'set')
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 1,
      isDisposed: () => false
    })

    mirror.schedule()
    vi.advanceTimersByTime(250)
    mirror.schedule()
    vi.advanceTimersByTime(250)
    // Two refreshes ran, but the identical text is written only once.
    expect(setSpy).toHaveBeenCalledTimes(1)
  })

  it('skips the refresh when disposed before the timer fires', () => {
    const liveRegion = document.createElement('div')
    const term = makeRows(['content'])
    let disposed = false
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 1,
      isDisposed: () => disposed
    })

    mirror.schedule()
    disposed = true
    vi.advanceTimersByTime(250)
    expect(liveRegion.textContent).toBe('')
  })

  it('dispose cancels a pending refresh', () => {
    const liveRegion = document.createElement('div')
    const term = makeRows(['content'])
    const mirror = createAtermA11yMirror({
      liveRegion,
      term,
      getRows: () => 1,
      isDisposed: () => false
    })

    mirror.schedule()
    mirror.dispose()
    vi.advanceTimersByTime(250)
    expect(liveRegion.textContent).toBe('')
  })
})
