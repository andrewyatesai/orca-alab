import { describe, expect, it, vi } from 'vitest'
import { createFederatedSearchController } from './federated-search-controller'
import type { FederatedPaneBatch, SearchSourceAdapter } from './federated-search-model'

const orderContext = (): {
  focusedPaneKey: null
  visiblePaneKeys: Set<string>
  outputRecency: () => number
} => ({
  focusedPaneKey: null,
  visiblePaneKeys: new Set<string>(),
  outputRecency: () => 0
})

function batch(paneKey: string, absRow: number): FederatedPaneBatch {
  const [tabId, leafId] = paneKey.split(':')
  return {
    paneRef: { tabId, leafId, paneKey, worktreeId: null, title: null },
    sessionId: null,
    source: 'live',
    matches: [{ absRow, col: 0, len: 3, snippet: null }],
    total: 1,
    incomplete: false,
    approxTime: null
  }
}

describe('createFederatedSearchController', () => {
  it('streams adapter batches into ranked groups and clears pending on completion', async () => {
    let emitBatch: ((b: FederatedPaneBatch) => void) | null = null
    let finish: (() => void) | null = null
    const adapter: SearchSourceAdapter = {
      query: (_q, _o, _g, _m, emit) =>
        new Promise((resolve) => {
          emitBatch = emit
          finish = resolve
        }),
      cancel: vi.fn()
    }
    const controller = createFederatedSearchController({ adapters: [adapter], orderContext })
    controller.setQuery('foo', { caseSensitive: false, isRegex: false })
    expect(controller.snapshot().pending).toBe(true)
    emitBatch!(batch('t1:a', 9))
    expect(controller.snapshot().groups).toHaveLength(1)
    finish!()
    await Promise.resolve()
    await Promise.resolve()
    expect(controller.snapshot().pending).toBe(false)
  })

  it('a stale-generation batch NEVER renders (keystroke bumps the generation)', async () => {
    const emitters: { gen: number; emit: (b: FederatedPaneBatch) => void }[] = []
    const adapter: SearchSourceAdapter = {
      query: (_q, _o, gen, _m, emit) =>
        new Promise(() => {
          emitters.push({ gen, emit })
        }),
      cancel: vi.fn()
    }
    const controller = createFederatedSearchController({ adapters: [adapter], orderContext })
    controller.setQuery('fo', { caseSensitive: false, isRegex: false })
    controller.setQuery('foo', { caseSensitive: false, isRegex: false })
    // The FIRST (superseded) generation delivers late — it must be dropped.
    emitters[0].emit(batch('t1:stale', 1))
    expect(controller.snapshot().groups).toHaveLength(0)
    // The live generation's batch renders.
    emitters[1].emit(batch('t1:live', 2))
    expect(controller.snapshot().groups.map((g) => g.paneRef?.paneKey)).toEqual(['t1:live'])
  })

  it('every new query cancels the previous generation on all adapters', () => {
    const cancel = vi.fn()
    const adapter: SearchSourceAdapter = {
      query: () => new Promise(() => undefined),
      cancel
    }
    const controller = createFederatedSearchController({ adapters: [adapter], orderContext })
    controller.setQuery('a', { caseSensitive: false, isRegex: false })
    controller.setQuery('ab', { caseSensitive: false, isRegex: false })
    expect(cancel).toHaveBeenCalledWith(1)
  })

  it('cancel() (Esc) cancels in-flight source queries and clears pending', () => {
    const cancel = vi.fn()
    const adapter: SearchSourceAdapter = {
      query: () => new Promise(() => undefined),
      cancel
    }
    const controller = createFederatedSearchController({ adapters: [adapter], orderContext })
    controller.setQuery('foo', { caseSensitive: false, isRegex: false })
    controller.cancel()
    expect(cancel).toHaveBeenLastCalledWith(1)
    expect(controller.snapshot().pending).toBe(false)
  })

  it('a rejecting adapter degrades to no results without throwing', async () => {
    const adapter: SearchSourceAdapter = {
      query: () => Promise.reject(new Error('source down')),
      cancel: vi.fn()
    }
    const controller = createFederatedSearchController({ adapters: [adapter], orderContext })
    controller.setQuery('foo', { caseSensitive: false, isRegex: false })
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
    expect(controller.snapshot().pending).toBe(false)
    expect(controller.snapshot().groups).toHaveLength(0)
  })

  it('enforces the depth-extension cutoff at merge time', () => {
    let emitBatch: ((b: FederatedPaneBatch) => void) | null = null
    const adapter: SearchSourceAdapter = {
      query: (_q, _o, _g, _m, emit) =>
        new Promise(() => {
          emitBatch = emit
        }),
      cancel: vi.fn()
    }
    const controller = createFederatedSearchController({
      adapters: [adapter],
      orderContext,
      depthExtensions: () => [{ sessionId: 's1', paneKey: 't1:a', cutoffRow: 400 }]
    })
    controller.setQuery('foo', { caseSensitive: false, isRegex: false })
    emitBatch!({
      ...batch('t1:a', 0),
      sessionId: 's1',
      source: 'daemon-history',
      depthExtension: true,
      matches: [
        { absRow: 100, col: 0, len: 3, snippet: null },
        { absRow: 450, col: 0, len: 3, snippet: null }
      ],
      total: 2
    })
    const group = controller.snapshot().groups[0]
    expect(group.matches.map((m) => m.absRow)).toEqual([100])
  })
})
