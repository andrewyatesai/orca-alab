import { describe, expect, it, vi } from 'vitest'
import type { DiscoveredLivePane } from './live-pane-search-adapter'

// Only the discovery seam is exercised here; the rest of the production wiring
// (store, worker, registry) is untouched by these order-context assertions.
const discoverLiveFederatedPanes = vi.fn<() => DiscoveredLivePane[]>()
vi.mock('./live-pane-discovery', () => ({
  discoverLiveFederatedPanes: () => discoverLiveFederatedPanes()
}))

const { productionOrderContext } = await import('./federated-search-production')

function pane(key: string, overrides: Partial<DiscoveredLivePane> = {}): DiscoveredLivePane {
  const [tabId, leafId] = key.split(':')
  return {
    paneRef: { tabId, leafId, paneKey: key, worktreeId: 'wt', title: null },
    visible: false,
    focused: false,
    sessionId: null,
    lastOutputAt: 0,
    target: {} as DiscoveredLivePane['target'],
    ...overrides
  }
}

describe('productionOrderContext', () => {
  it('resolves recency via an O(1) index — no per-lookup scan of the pane array', () => {
    const panes = [
      pane('t1:a', { lastOutputAt: 10 }),
      pane('t1:b', { lastOutputAt: 20 }),
      pane('t1:c', { lastOutputAt: 30 })
    ]
    const findSpy = vi.spyOn(panes, 'find')
    discoverLiveFederatedPanes.mockReturnValue(panes)

    const ctx = productionOrderContext()
    // Context construction may scan once (to locate the focused pane); after
    // that, recency lookups must never rescan the array.
    findSpy.mockClear()

    for (let i = 0; i < 50; i++) {
      expect(ctx.outputRecency('t1:a')).toBe(10)
      expect(ctx.outputRecency('t1:c')).toBe(30)
      expect(ctx.outputRecency('missing')).toBe(0)
    }

    expect(findSpy).not.toHaveBeenCalled()
  })

  it('maps focused/visible/recency from discovered panes', () => {
    const panes = [
      pane('t1:a', { focused: true, visible: true, lastOutputAt: 5 }),
      pane('t1:b', { visible: true, lastOutputAt: 7 }),
      pane('t1:c', { lastOutputAt: 9 })
    ]
    discoverLiveFederatedPanes.mockReturnValue(panes)

    const ctx = productionOrderContext()

    expect(ctx.focusedPaneKey).toBe('t1:a')
    expect([...ctx.visiblePaneKeys].sort()).toEqual(['t1:a', 't1:b'])
    expect(ctx.outputRecency('t1:b')).toBe(7)
    expect(ctx.outputRecency('t1:c')).toBe(9)
  })
})
