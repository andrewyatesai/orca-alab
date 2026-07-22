import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWorkerSearch, SEARCH_REFRESH_TICK_MS } from './aterm-worker-search'

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

// Minimal engine/handle stub. `search` returns one match (flat [line,startCol,length]).
function makeHandle() {
  const engine = {
    scroll_search_line_into_view: vi.fn(),
    search_display_origin: 0,
    display_offset: 0,
    cell_width: 8,
    cell_height: 16
  }
  const search = vi.fn(() => new Uint32Array([3, 0, 4]))
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return { handle: { engine, search } as any, search }
}

/** Pin performance.now to a mutable clock the `search` stub advances, simulating a
 *  rebuild whose measured cost the P6 gate reads. Install AFTER vi.useFakeTimers so
 *  the spy wins regardless of what the fake-timer install touches. */
function makeSlowClock(costMs: number, search: ReturnType<typeof vi.fn>): { now: () => number } {
  const clock = { value: 0 }
  vi.spyOn(performance, 'now').mockImplementation(() => clock.value)
  search.mockImplementation(() => {
    clock.value += costMs
    return new Uint32Array([3, 0, 4])
  })
  return { now: () => clock.value }
}

describe('aterm-worker-search dirty coalescing', () => {
  it('re-indexes once per read regardless of how many markDirty calls precede it', () => {
    const { handle, search } = makeHandle()
    const s = createWorkerSearch(handle, () => 24)

    s.find('foo', false, false)
    expect(search).toHaveBeenCalledTimes(1) // find indexes once

    // Simulate many PTY chunks: mark dirty repeatedly, NO re-index yet.
    for (let i = 0; i < 10; i++) {
      s.markDirty()
    }
    expect(search).toHaveBeenCalledTimes(1)

    // The first read of the frame coalesces all 10 into ONE re-index...
    expect(s.count()).toBe(1)
    expect(search).toHaveBeenCalledTimes(2)
    // ...and further reads in the same frame (no new markDirty) don't re-index.
    s.activeIndex()
    s.activeRect()
    s.visibleRects()
    expect(search).toHaveBeenCalledTimes(2)
  })

  it('re-indexes again only after a fresh markDirty', () => {
    const { handle, search } = makeHandle()
    const s = createWorkerSearch(handle, () => 24)
    s.find('foo', false, false)
    s.count() // no dirty pending → no extra index
    expect(search).toHaveBeenCalledTimes(1)
    s.markDirty()
    s.count()
    expect(search).toHaveBeenCalledTimes(2)
  })

  it('flushes a pending re-index before search navigation', () => {
    const { handle, search } = makeHandle()
    const s = createWorkerSearch(handle, () => 24)
    s.find('foo', false, false)
    s.markDirty()
    s.next()
    expect(search).toHaveBeenCalledTimes(2) // next() flushed the dirty index
  })

  it('markDirty does nothing without an active query', () => {
    const { handle, search } = makeHandle()
    const s = createWorkerSearch(handle, () => 24)
    s.markDirty()
    expect(s.count()).toBe(0)
    expect(search).not.toHaveBeenCalled() // no query → no index
  })

  it('find and clear cancel a pending dirty', () => {
    const { handle, search } = makeHandle()
    const s = createWorkerSearch(handle, () => 24)
    s.find('foo', false, false)
    s.markDirty()
    s.clear() // resets, no query
    s.count()
    expect(search).toHaveBeenCalledTimes(1) // only the initial find indexed
  })
})

// P6 interim streaming rule: an index whose LAST rebuild exceeded the refresh tick is
// NOT rebuilt per streaming frame — reads serve the flagged stale results and the
// trailing timer lands the guaranteed final re-index.
describe('aterm-worker-search cost gate', () => {
  it('skips the per-frame re-index and flags stale when the last rebuild exceeded the tick', () => {
    vi.useFakeTimers()
    const { handle, search } = makeHandle()
    makeSlowClock(SEARCH_REFRESH_TICK_MS * 5, search)
    const s = createWorkerSearch(handle, () => 24)

    s.find('foo', false, false) // rebuild measured at 5x the tick
    expect(s.resultsStale()).toBe(false)

    s.markDirty()
    // Streaming frame reads: served from the OLD results, no rebuild, flagged stale.
    expect(s.count()).toBe(1)
    expect(search).toHaveBeenCalledTimes(1)
    expect(s.resultsStale()).toBe(true)
  })

  it('lands the guaranteed final refresh via the trailing timer and notifies the owner', () => {
    vi.useFakeTimers()
    const { handle, search } = makeHandle()
    makeSlowClock(SEARCH_REFRESH_TICK_MS * 5, search)
    const onAsyncRefresh = vi.fn()
    const s = createWorkerSearch(handle, () => 24, onAsyncRefresh)

    s.find('foo', false, false)
    const versionBefore = s.resultsVersion()
    s.markDirty()
    s.count() // gate skips + arms the trailing timer
    expect(s.resultsStale()).toBe(true)

    // Cost-proportional delay: max(tick, lastRebuildMs) — advance past it.
    vi.advanceTimersByTime(SEARCH_REFRESH_TICK_MS * 5)
    expect(search).toHaveBeenCalledTimes(2) // the final re-index ran
    expect(s.resultsStale()).toBe(false)
    expect(s.resultsVersion()).toBeGreaterThan(versionBefore) // result versioning
    expect(onAsyncRefresh).toHaveBeenCalledTimes(1) // owner posts the fresh STATE
  })

  it('a NEW find is never cost-gated (user-initiated searches run immediately)', () => {
    vi.useFakeTimers()
    const { handle, search } = makeHandle()
    makeSlowClock(SEARCH_REFRESH_TICK_MS * 5, search)
    const s = createWorkerSearch(handle, () => 24)

    s.find('foo', false, false)
    s.markDirty()
    s.count() // gate skips, stale
    s.find('bar', false, false) // new query: runs now, clears stale
    expect(search).toHaveBeenCalledTimes(2)
    expect(s.resultsStale()).toBe(false)
  })

  it('a cheap index (rebuild <= tick) keeps the immediate per-frame re-index', () => {
    vi.useFakeTimers()
    const { handle, search } = makeHandle()
    const s = createWorkerSearch(handle, () => 24)

    s.find('foo', false, false)
    s.markDirty()
    s.count()
    expect(search).toHaveBeenCalledTimes(2) // re-indexed inline, no gate
    expect(s.resultsStale()).toBe(false)
  })

  it('dispose cancels the armed trailing timer (no re-index against a freed engine)', () => {
    vi.useFakeTimers()
    const { handle, search } = makeHandle()
    makeSlowClock(SEARCH_REFRESH_TICK_MS * 5, search)
    const onAsyncRefresh = vi.fn()
    const s = createWorkerSearch(handle, () => 24, onAsyncRefresh)

    s.find('foo', false, false)
    s.markDirty()
    s.count() // arms the timer
    s.dispose()
    vi.advanceTimersByTime(SEARCH_REFRESH_TICK_MS * 10)
    expect(search).toHaveBeenCalledTimes(1)
    expect(onAsyncRefresh).not.toHaveBeenCalled()
  })
})
