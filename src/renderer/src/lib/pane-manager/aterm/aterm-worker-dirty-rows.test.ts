import { describe, expect, it, vi } from 'vitest'
import {
  ATERM_GRID_MIRROR_CHURN_SYNC_INTERVAL_MS,
  createAtermDirtyRowTracker
} from './aterm-worker-dirty-rows'

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

    const dirty = tracker.build(2, cols, 0)

    expect(dirty.map((r) => r.widths)).toEqual(['11111111', '11111111'])
    // The fast path must NOT cross the wasm boundary for width per cell.
    expect(engine.cell_is_wide).not.toHaveBeenCalled()
  })

  it('falls back to the per-cell walk for rows with non-ASCII and reports wide leads', () => {
    const cols = 4
    // Row 0 has a wide CJK char whose lead cell is column 1.
    const engine = makeEngine(['a漢b'], { 0: new Set([1]) })
    const tracker = createAtermDirtyRowTracker(engine)

    const dirty = tracker.build(1, cols, 0)

    expect(dirty[0].widths).toBe('1211')
    expect(engine.cell_is_wide).toHaveBeenCalled()
  })

  it('produces byte-identical widths to the per-cell walk for ASCII (1:1 column mapping)', () => {
    const cols = 6
    const engine = makeEngine(['abc'])
    const tracker = createAtermDirtyRowTracker(engine)
    const fast = tracker.build(1, cols, 0)[0].widths
    // What the old per-cell walk would produce for an all-ASCII row: all '1'.
    const perCell = Array.from({ length: cols }, () => '1').join('')
    expect(fast).toBe(perCell)
  })

  it('rebuilds the cached width string when cols changes (for a dirty row)', () => {
    const rows = ['x']
    const engine = makeEngine(rows)
    const tracker = createAtermDirtyRowTracker(engine)
    expect(tracker.build(1, 3, 0)[0].widths).toBe('111')
    // Change the row text so it re-emits at the wider grid → cached string grows.
    rows[0] = 'xy'
    expect(tracker.build(1, 5, 0)[0].widths).toBe('11111')
  })
})

// ── P7: rate-limit the row export during rapid display_offset churn ──────────────
// Only the grid-row mirror is throttled (the caller keeps all STATE scalars live);
// stale() flags a withheld export so the frame scheduler owes a settle sync.
describe('aterm-worker-dirty-rows churn rate limit (P7)', () => {
  const INTERVAL = ATERM_GRID_MIRROR_CHURN_SYNC_INTERVAL_MS

  function makeClock(): { now: () => number; advance: (ms: number) => void } {
    let t = 0
    return {
      now: () => t,
      advance: (ms) => {
        t += ms
      }
    }
  }

  it('a single offset step (one wheel notch) is never throttled', () => {
    const rows = ['a', 'b']
    const engine = makeEngine(rows)
    const clock = makeClock()
    const tracker = createAtermDirtyRowTracker(engine, clock.now)
    tracker.build(2, 4, 0)
    clock.advance(16)
    rows[0] = 'scrolled'
    // First churn frame: full export (streak of 1 is a notch, not a fling).
    expect(tracker.build(2, 4, 5).map((r) => r.text)).toEqual(['scrolled'])
    expect(tracker.stale()).toBe(false)
  })

  it('sustained churn within the window withholds the export entirely and flags stale', () => {
    const rows = ['a', 'b']
    const engine = makeEngine(rows)
    const clock = makeClock()
    const tracker = createAtermDirtyRowTracker(engine, clock.now)
    tracker.build(2, 4, 0)
    clock.advance(16)
    tracker.build(2, 4, 5) // churn frame 1 → full export, resets the window
    engine.row_text.mockClear()
    clock.advance(16)
    rows[0] = 'fling-a'
    expect(tracker.build(2, 4, 10)).toEqual([]) // churn frame 2 → throttled
    clock.advance(16)
    rows[0] = 'fling-b'
    expect(tracker.build(2, 4, 15)).toEqual([]) // churn frame 3 → throttled
    // The throttled frames must not touch the wasm boundary at all.
    expect(engine.row_text).not.toHaveBeenCalled()
    expect(tracker.stale()).toBe(true)
  })

  it('churning past the sync window re-exports (rate-limited, not suppressed)', () => {
    const rows = ['a']
    const engine = makeEngine(rows)
    const clock = makeClock()
    const tracker = createAtermDirtyRowTracker(engine, clock.now)
    tracker.build(1, 4, 0)
    clock.advance(16)
    tracker.build(1, 4, 5)
    clock.advance(16)
    rows[0] = 'mid-fling'
    expect(tracker.build(1, 4, 10)).toEqual([])
    // Cross the window while still churning → the next frame syncs in full.
    clock.advance(INTERVAL)
    expect(tracker.build(1, 4, 15).map((r) => r.text)).toEqual(['mid-fling'])
    expect(tracker.stale()).toBe(false)
  })

  it('the settle frame (offset unchanged) always exports in full and clears stale', () => {
    const rows = ['a', 'b']
    const engine = makeEngine(rows)
    const clock = makeClock()
    const tracker = createAtermDirtyRowTracker(engine, clock.now)
    tracker.build(2, 4, 0)
    clock.advance(16)
    tracker.build(2, 4, 5)
    clock.advance(16)
    rows[0] = 'final-a'
    rows[1] = 'final-b'
    expect(tracker.build(2, 4, 10)).toEqual([])
    expect(tracker.stale()).toBe(true)
    // Settle: same offset, still inside the window — must sync regardless.
    clock.advance(1)
    expect(tracker.build(2, 4, 10).map((r) => r.text)).toEqual(['final-a', 'final-b'])
    expect(tracker.stale()).toBe(false)
  })

  it('a rows-count change mid-churn forces a full export (resize is not a fling)', () => {
    const rows = ['a', 'b', 'c']
    const engine = makeEngine(rows)
    const clock = makeClock()
    const tracker = createAtermDirtyRowTracker(engine, clock.now)
    tracker.build(2, 4, 0)
    clock.advance(16)
    tracker.build(2, 4, 5)
    clock.advance(16)
    expect(tracker.build(2, 4, 10)).toEqual([])
    clock.advance(1)
    // Still churning + inside the window, but the viewport grew → full export.
    expect(tracker.build(3, 4, 15)).toHaveLength(3)
    expect(tracker.stale()).toBe(false)
  })

  it('non-churn frames (typing at a fixed offset) are never throttled', () => {
    const rows = ['a']
    const engine = makeEngine(rows)
    const clock = makeClock()
    const tracker = createAtermDirtyRowTracker(engine, clock.now)
    tracker.build(1, 4, 0)
    for (const text of ['ab', 'abc', 'abcd']) {
      clock.advance(8)
      rows[0] = text
      expect(tracker.build(1, 4, 0).map((r) => r.text)).toEqual([text])
      expect(tracker.stale()).toBe(false)
    }
  })
})

// ── E9: single batch row-range export replaces the per-row wasm walk ─────────────
// Feature-detected on the pinned artifact; the per-row path above stays the
// fallback for engines without the export (and for skewed payloads).
describe('aterm-worker-dirty-rows E9 batch row-range export', () => {
  // Engine whose batch export mirrors its per-row surface, so both paths must
  // produce identical dirty rows. `widths` is emitted only for rows with a wide cell.
  function makeBatchEngine(rows: string[], wideCols: Record<number, Set<number>> = {}) {
    const perRow = makeEngine(rows, wideCols)
    const cols = { value: 0 }
    return {
      ...perRow,
      cols,
      row_range_json: vi.fn((first: number, count: number) =>
        JSON.stringify(
          Array.from({ length: count }, (_, i) => {
            const y = first + i
            const text = rows[y] ?? ''
            const wide = wideCols[y]
            const base = { text, wrapped: false, len: text.length }
            return wide && wide.size > 0
              ? {
                  ...base,
                  widths: Array.from({ length: cols.value }, (_, x) =>
                    wide.has(x) ? '2' : '1'
                  ).join('')
                }
              : base
          })
        )
      )
    }
  }

  it('one boundary crossing per build: no per-row or per-cell engine calls', () => {
    const engine = makeBatchEngine(['hello', 'wo漢ld'], { 1: new Set([2]) })
    engine.cols.value = 8
    const tracker = createAtermDirtyRowTracker(engine)
    const dirty = tracker.build(2, 8, 0)
    expect(engine.row_range_json).toHaveBeenCalledExactlyOnceWith(0, 2)
    expect(engine.row_text).not.toHaveBeenCalled()
    expect(engine.row_is_wrapped).not.toHaveBeenCalled()
    expect(engine.row_len).not.toHaveBeenCalled()
    expect(engine.cell_is_wide).not.toHaveBeenCalled()
    expect(dirty.map((r) => r.widths)).toEqual(['11111111', '11211111'])
  })

  it('produces dirty rows identical to the per-row path for the same content', () => {
    const rows = ['plain', 'wi漢de', '']
    const wide = { 1: new Set([2]) }
    const batchEngine = makeBatchEngine(rows, wide)
    batchEngine.cols.value = 6
    const perRowEngine = makeEngine(rows, wide)
    const fromBatch = createAtermDirtyRowTracker(batchEngine).build(3, 6, 0)
    const fromPerRow = createAtermDirtyRowTracker(perRowEngine).build(3, 6, 0)
    expect(fromBatch).toEqual(fromPerRow)
  })

  it('unchanged rows are still diffed away across builds (batch path keeps deltas)', () => {
    const rows = ['a', 'b']
    const engine = makeBatchEngine(rows)
    engine.cols.value = 4
    const tracker = createAtermDirtyRowTracker(engine)
    expect(tracker.build(2, 4, 0)).toHaveLength(2)
    rows[1] = 'B!'
    const dirty = tracker.build(2, 4, 0)
    expect(dirty.map((r) => ({ y: r.y, text: r.text }))).toEqual([{ y: 1, text: 'B!' }])
  })

  it('churn-throttled frames never touch the batch export either', () => {
    const rows = ['a']
    const engine = makeBatchEngine(rows)
    engine.cols.value = 4
    let t = 0
    const tracker = createAtermDirtyRowTracker(engine, () => t)
    tracker.build(1, 4, 0)
    t += 16
    tracker.build(1, 4, 5) // churn frame 1 → full export
    engine.row_range_json.mockClear()
    t += 16
    expect(tracker.build(1, 4, 10)).toEqual([]) // churn frame 2 → throttled
    expect(engine.row_range_json).not.toHaveBeenCalled()
    expect(tracker.stale()).toBe(true)
  })

  it('an undefined export (range unavailable) falls back to per-row for that frame only', () => {
    const rows = ['abc']
    const engine = makeBatchEngine(rows)
    engine.cols.value = 4
    engine.row_range_json.mockReturnValueOnce(undefined as unknown as string)
    const tracker = createAtermDirtyRowTracker(engine)
    expect(tracker.build(1, 4, 0).map((r) => r.text)).toEqual(['abc'])
    expect(engine.row_text).toHaveBeenCalled()
    // Next build retries — and uses — the batch export again.
    engine.row_text.mockClear()
    rows[0] = 'abcd'
    expect(tracker.build(1, 4, 0).map((r) => r.text)).toEqual(['abcd'])
    expect(engine.row_text).not.toHaveBeenCalled()
  })

  it('a skewed payload disables the batch path for good and serves per-row rows', () => {
    const rows = ['abc']
    const engine = makeBatchEngine(rows)
    engine.cols.value = 4
    engine.row_range_json.mockReturnValue('skewed{')
    const tracker = createAtermDirtyRowTracker(engine)
    expect(tracker.build(1, 4, 0).map((r) => r.text)).toEqual(['abc'])
    rows[0] = 'abcd'
    expect(tracker.build(1, 4, 0).map((r) => r.text)).toEqual(['abcd'])
    expect(engine.row_range_json).toHaveBeenCalledTimes(1)
    expect(engine.row_text).toHaveBeenCalled()
  })
})
