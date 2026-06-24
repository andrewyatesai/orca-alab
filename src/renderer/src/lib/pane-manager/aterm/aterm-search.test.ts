import { describe, expect, it, vi } from 'vitest'
import { createAtermSearchController } from './aterm-search'
import type { AtermTerminal } from './aterm_wasm.js'

// A minimal AtermTerminal stand-in: only `search` is exercised by the controller.
// `search` returns a flat [line, startCol, len, …] Uint32Array, mirroring the wasm.
function fakeTerm(
  searchImpl: (q: string, cs: boolean, rx: boolean) => number[]
): AtermTerminal {
  return {
    search: (q: string, cs: boolean, rx: boolean) => new Uint32Array(searchImpl(q, cs, rx))
  } as unknown as AtermTerminal
}

describe('createAtermSearchController', () => {
  it('decodes the FINAL triplet (no off-by-one drop)', () => {
    // Two matches → six u32s; the last triplet ends at the final index and must
    // not be dropped (regression: the old `i + 2 < length` bound dropped it).
    const term = fakeTerm(() => [3, 0, 5, 7, 2, 4])
    const setSearchHighlights = vi.fn()
    const controller = createAtermSearchController(term, {
      setSearchHighlights,
      scrollToMatch: vi.fn(),
      redraw: vi.fn()
    })
    const count = controller.find('q', true, false)
    expect(count).toBe(2)
    const [matches] = setSearchHighlights.mock.calls.at(-1) ?? []
    expect(matches).toEqual([
      { line: 3, startCol: 0, length: 5 },
      { line: 7, startCol: 2, length: 4 }
    ])
  })

  it('reports an active query and re-runs it on refresh against new content', () => {
    let rows: number[] = [1, 0, 3]
    const term = fakeTerm(() => rows)
    const setSearchHighlights = vi.fn()
    const controller = createAtermSearchController(term, {
      setSearchHighlights,
      scrollToMatch: vi.fn(),
      redraw: vi.fn()
    })

    expect(controller.hasActiveQuery()).toBe(false)
    controller.find('tok', false, false)
    expect(controller.hasActiveQuery()).toBe(true)
    expect(controller.count()).toBe(1)

    // Simulate new output shifting the match to a different absolute row.
    rows = [42, 0, 3]
    controller.refresh()
    const [matches] = setSearchHighlights.mock.calls.at(-1) ?? []
    expect(matches).toEqual([{ line: 42, startCol: 0, length: 3 }])
  })

  it('clears the active query and highlights on clear()', () => {
    const term = fakeTerm(() => [0, 0, 2])
    const setSearchHighlights = vi.fn()
    const controller = createAtermSearchController(term, {
      setSearchHighlights,
      scrollToMatch: vi.fn(),
      redraw: vi.fn()
    })
    controller.find('q', false, false)
    controller.clear()
    expect(controller.hasActiveQuery()).toBe(false)
    expect(controller.count()).toBe(0)
    expect(setSearchHighlights).toHaveBeenLastCalledWith([], -1)
  })

  it('forwards the case + regex flags to the engine on find and refresh', () => {
    const search = vi.fn(() => new Uint32Array([0, 0, 2]))
    const term = { search } as unknown as AtermTerminal
    const controller = createAtermSearchController(term, {
      setSearchHighlights: vi.fn(),
      scrollToMatch: vi.fn(),
      redraw: vi.fn()
    })
    controller.find('a.+b', true, true)
    expect(search).toHaveBeenLastCalledWith('a.+b', true, true)
    // refresh re-runs the SAME query with the stored case + regex flags.
    controller.refresh()
    expect(search).toHaveBeenLastCalledWith('a.+b', true, true)
  })

  it('refresh is a no-op when no query is active', () => {
    const search = vi.fn(() => new Uint32Array())
    const term = { search } as unknown as AtermTerminal
    const controller = createAtermSearchController(term, {
      setSearchHighlights: vi.fn(),
      scrollToMatch: vi.fn(),
      redraw: vi.fn()
    })
    controller.refresh()
    expect(search).not.toHaveBeenCalled()
  })
})
