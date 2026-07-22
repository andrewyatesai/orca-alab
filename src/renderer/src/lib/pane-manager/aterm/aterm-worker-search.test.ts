import { describe, expect, it, vi } from 'vitest'
import { createWorkerSearch } from './aterm-worker-search'

// Minimal engine/handle stub. `search` returns one match (flat [line,startCol,length]).
function makeHandle(matches: number[] = [3, 0, 4]) {
  const engine = {
    scroll_search_line_into_view: vi.fn(),
    search_display_origin: 76,
    display_offset: 0,
    base_y: 76,
    cell_width: 8,
    cell_height: 16
  }
  const search = vi.fn(() => new Uint32Array(matches))
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return { handle: { engine, search } as any, search, engine }
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

describe('aterm-worker-search find-generation echo', () => {
  it('echoes the last find generation (0 before any find / when omitted)', () => {
    const { handle } = makeHandle()
    const s = createWorkerSearch(handle, () => 24)
    expect(s.generation()).toBe(0)
    s.find('foo', false, false, 7)
    expect(s.generation()).toBe(7)
    s.find('foobar', false, false)
    expect(s.generation()).toBe(0)
  })
})

describe('aterm-worker-search scrollbar marker model', () => {
  it('derives bounded fractions from the full sorted match list', () => {
    // base_y=76 + 24 rows → 100 retained lines; matches at lines 10 and 60.
    const { handle } = makeHandle([10, 0, 4, 60, 2, 4])
    const s = createWorkerSearch(handle, () => 24)
    s.find('foo', false, false)
    const model = s.markerModel()
    expect(model.fractions).toEqual([0.105, 0.605])
    // find selects the LAST match → its fraction is active.
    expect(model.activeFraction).toBe(0.605)
  })

  it('re-indexes a dirty query before deriving markers, and memoizes across frames', () => {
    const { handle, search } = makeHandle([10, 0, 4])
    const s = createWorkerSearch(handle, () => 24)
    s.find('foo', false, false)
    const first = s.markerModel()
    expect(s.markerModel()).toBe(first) // unchanged frame → cached model identity
    s.markDirty()
    s.markerModel()
    expect(search).toHaveBeenCalledTimes(2) // markerModel flushed the dirty index
  })

  it('is empty with no query and after clear', () => {
    const { handle } = makeHandle([10, 0, 4])
    const s = createWorkerSearch(handle, () => 24)
    expect(s.markerModel().fractions).toEqual([])
    s.find('foo', false, false)
    expect(s.markerModel().fractions).toHaveLength(1)
    s.clear()
    expect(s.markerModel()).toEqual({ fractions: [], activeFraction: null })
  })
})
