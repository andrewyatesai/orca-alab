import { describe, expect, it, vi } from 'vitest'
import { createWorkerSearch } from './aterm-worker-search'

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
