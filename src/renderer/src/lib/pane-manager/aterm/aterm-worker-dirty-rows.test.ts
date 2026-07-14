import { describe, expect, it, vi } from 'vitest'
import { createAtermDirtyRowTracker } from './aterm-worker-dirty-rows'

// A minimal engine stub: `rows` of text, wide-cell columns per row.
function makeEngine(rows: string[], wideCols: Record<number, Set<number>> = {}) {
  return {
    row_text: vi.fn((y: number) => rows[y] ?? ''),
    row_is_wrapped: vi.fn(() => false),
    row_len: vi.fn((y: number) => (rows[y] ?? '').length),
    cell_is_wide: vi.fn((y: number, x: number) => wideCols[y]?.has(x) === true)
  }
}

describe('aterm-worker-dirty-rows ASCII fast-path', () => {
  it('skips the per-cell cell_is_wide walk for all-ASCII rows and emits all-1 widths', () => {
    const cols = 8
    const engine = makeEngine(['hello', 'world'])
    const tracker = createAtermDirtyRowTracker(engine)

    const dirty = tracker.build(2, cols)

    expect(dirty.map((r) => r.widths)).toEqual(['11111111', '11111111'])
    // The fast path must NOT cross the wasm boundary for width per cell.
    expect(engine.cell_is_wide).not.toHaveBeenCalled()
  })

  it('falls back to the per-cell walk for rows with non-ASCII and reports wide leads', () => {
    const cols = 4
    // Row 0 has a wide CJK char whose lead cell is column 1.
    const engine = makeEngine(['a漢b'], { 0: new Set([1]) })
    const tracker = createAtermDirtyRowTracker(engine)

    const dirty = tracker.build(1, cols)

    expect(dirty[0].widths).toBe('1211')
    expect(engine.cell_is_wide).toHaveBeenCalled()
  })

  it('produces byte-identical widths to the per-cell walk for ASCII (1:1 column mapping)', () => {
    const cols = 6
    const engine = makeEngine(['abc'])
    const tracker = createAtermDirtyRowTracker(engine)
    const fast = tracker.build(1, cols)[0].widths
    // What the old per-cell walk would produce for an all-ASCII row: all '1'.
    const perCell = Array.from({ length: cols }, () => '1').join('')
    expect(fast).toBe(perCell)
  })

  it('rebuilds the cached width string when cols changes (for a dirty row)', () => {
    const rows = ['x']
    const engine = makeEngine(rows)
    const tracker = createAtermDirtyRowTracker(engine)
    expect(tracker.build(1, 3)[0].widths).toBe('111')
    // Change the row text so it re-emits at the wider grid → cached string grows.
    rows[0] = 'xy'
    expect(tracker.build(1, 5)[0].widths).toBe('11111')
  })
})
