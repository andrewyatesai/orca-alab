/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { createAtermPanePresenter } from './aterm-pane-present'

// The stage-3 presenter seam: the spill pass runs directly after drawFrame on
// BOTH paint paths — the rAF draw and the eager presentNow — so a keystroke
// echo's ring reaches the overlay in the same paint (no one-frame lag), and a
// reconcile-consumed frame (which paints nothing) runs no spill pass either.

function makePresenter(overrides: { reconcile?: () => boolean } = {}): {
  presenter: ReturnType<typeof createAtermPanePresenter>
  calls: string[]
} {
  const calls: string[] = []
  const presenter = createAtermPanePresenter({
    strategy: { drawFrame: () => void calls.push('drawFrame') },
    searchOverlay: null,
    a11yMirror: { schedule: () => undefined },
    gridReflow: { reconcileIfNeeded: overrides.reconcile ?? (() => false) },
    drawScheduler: {
      consume: () => undefined,
      schedule: () => undefined,
      isSuspended: () => false
    },
    scheduleDraw: () => undefined,
    isDisposed: () => false,
    getSearchMatches: () => [],
    getSearchActiveIndex: () => -1,
    effectsDrive: { beforeFrame: () => undefined, afterFrame: () => undefined },
    spillBlit: () => void calls.push('spillBlit')
  })
  return { presenter, calls }
}

describe('presenter spill seam', () => {
  it('runs the spill pass right after drawFrame on the rAF draw path', () => {
    const { presenter, calls } = makePresenter()
    presenter.draw()
    expect(calls).toEqual(['drawFrame', 'spillBlit'])
  })

  it('runs the spill pass on the eager presentNow path too', () => {
    vi.stubGlobal('requestAnimationFrame', vi.fn())
    const { presenter, calls } = makePresenter()
    presenter.presentNow()
    expect(calls).toEqual(['drawFrame', 'spillBlit'])
    vi.unstubAllGlobals()
  })

  it('a reconcile-consumed frame paints nothing and blits nothing', () => {
    const { presenter, calls } = makePresenter({ reconcile: () => true })
    presenter.draw()
    expect(calls).toEqual([])
  })

  it('tolerates the dep being unset (worker path / non-exporting engines)', () => {
    const calls: string[] = []
    const presenter = createAtermPanePresenter({
      strategy: { drawFrame: () => void calls.push('drawFrame') },
      searchOverlay: null,
      a11yMirror: { schedule: () => undefined },
      gridReflow: { reconcileIfNeeded: () => false },
      drawScheduler: {
        consume: () => undefined,
        schedule: () => undefined,
        isSuspended: () => false
      },
      scheduleDraw: () => undefined,
      isDisposed: () => false,
      getSearchMatches: () => [],
      getSearchActiveIndex: () => -1,
      effectsDrive: { beforeFrame: () => undefined, afterFrame: () => undefined }
    })
    presenter.draw()
    expect(calls).toEqual(['drawFrame'])
  })
})
