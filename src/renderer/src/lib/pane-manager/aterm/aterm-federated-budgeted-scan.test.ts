import { describe, expect, it, vi } from 'vitest'
import {
  runFederatedPaneScan,
  type FederatedScanEngine,
  type FederatedPaneScanResult
} from './aterm-federated-budgeted-scan'
import type { EngineBudgetedSearchStep } from './aterm-engine-budgeted-search'

// Synchronous yield keeps the sliced loop deterministic in tests.
const syncYield = (next: () => void): void => next()

function step(partial: Partial<EngineBudgetedSearchStep>): EngineBudgetedSearchStep {
  return {
    matches: new Uint32Array(),
    complete: false,
    cursor: 1n,
    reset: false,
    incompleteIndex: false,
    rowsFed: 0,
    totalRows: 100,
    ...partial
  }
}

function runToResult(
  engine: FederatedScanEngine,
  opts?: { maxMatches?: number; isCancelled?: () => boolean }
): FederatedPaneScanResult | null | 'unsettled' {
  let result: FederatedPaneScanResult | null | 'unsettled' = 'unsettled'
  runFederatedPaneScan({
    engine,
    query: 'foo',
    caseSensitive: false,
    isRegex: false,
    maxMatches: opts?.maxMatches ?? 50,
    isCancelled: opts?.isCancelled ?? (() => false),
    yieldSlice: syncYield,
    onDone: (r) => {
      result = r
    }
  })
  return result
}

describe('runFederatedPaneScan', () => {
  it('accumulates match deltas across slices and orders them newest-first', () => {
    const steps = [
      step({ matches: new Uint32Array([3, 0, 3, 7, 2, 3]), reset: true }),
      step({ matches: new Uint32Array([12, 1, 3]), complete: true, cursor: undefined })
    ]
    const engine: FederatedScanEngine = {
      searchBudgeted: vi.fn(() => steps.shift()!)
    }
    const result = runToResult(engine)
    expect(result).not.toBe('unsettled')
    expect(result).not.toBeNull()
    const r = result as FederatedPaneScanResult
    expect(r.matches.map((m) => m.absRow)).toEqual([12, 7, 3])
    expect(r.total).toBe(3)
    expect(r.incomplete).toBe(false)
    expect(r.matches.every((m) => m.snippet === null)).toBe(true)
  })

  it('caps streamed matches at maxMatches, keeping the NEWEST, with total honest', () => {
    const engine: FederatedScanEngine = {
      searchBudgeted: vi.fn(() =>
        step({
          matches: new Uint32Array([1, 0, 1, 2, 0, 1, 3, 0, 1, 4, 0, 1]),
          reset: true,
          complete: true,
          cursor: undefined
        })
      )
    }
    const r = runToResult(engine, { maxMatches: 2 }) as FederatedPaneScanResult
    expect(r.matches.map((m) => m.absRow)).toEqual([4, 3])
    expect(r.total).toBe(4)
  })

  it('drops accumulated deltas on a reset step (engine restarted the search)', () => {
    const steps = [
      step({ matches: new Uint32Array([1, 0, 1]), reset: true }),
      // Content changed: engine restarts from row zero on the resumed cursor.
      step({ matches: new Uint32Array([9, 0, 1]), reset: true, complete: true, cursor: undefined })
    ]
    const engine: FederatedScanEngine = {
      searchBudgeted: vi.fn(() => steps.shift()!)
    }
    const r = runToResult(engine) as FederatedPaneScanResult
    expect(r.matches.map((m) => m.absRow)).toEqual([9])
  })

  it('settles INCOMPLETE after the restart cap instead of scanning forever', () => {
    const searchBudgeted = vi.fn((_q, _cs, _r, cursor: bigint | undefined) =>
      // Every resumed slice restarts (sustained streaming) — never completes.
      step({ matches: new Uint32Array([1, 0, 1]), reset: true, cursor: (cursor ?? 0n) + 1n })
    )
    const cancel = vi.fn()
    const engine: FederatedScanEngine = {
      searchBudgeted,
      searchBudgetedCancel: cancel
    }
    const r = runToResult(engine) as FederatedPaneScanResult
    expect(r.incomplete).toBe(true)
    // Bounded: restart cap (3) means at most 4 slices ran.
    expect(searchBudgeted.mock.calls.length).toBeLessThanOrEqual(4)
    // The settle freed the partial index like a cancel would.
    expect(cancel).toHaveBeenCalled()
  })

  it('cancellation between slices frees the partial index and settles null', () => {
    let calls = 0
    const cancel = vi.fn()
    const engine: FederatedScanEngine = {
      searchBudgeted: vi.fn(() => {
        calls++
        return step({ matches: new Uint32Array([1, 0, 1]), reset: calls === 1 })
      }),
      searchBudgetedCancel: cancel
    }
    const result = runToResult(engine, { isCancelled: () => calls >= 2 })
    expect(result).toBeNull()
    expect(cancel).toHaveBeenCalled()
  })

  it('REFUSES a pin without the budgeted API: empty + incomplete, no engine call (E-6)', () => {
    // No searchBudgeted member at all — the E-6 binding contract says the scan
    // must fail closed rather than reach for any unbudgeted one-shot.
    const engine: FederatedScanEngine = {}
    const r = runToResult(engine) as FederatedPaneScanResult
    expect(r.matches).toEqual([])
    expect(r.total).toBe(0)
    expect(r.incomplete).toBe(true)
  })

  it('leaves snippets null on completion (no unbudgeted summary call exists to make)', () => {
    const engine: FederatedScanEngine = {
      searchBudgeted: vi.fn(() =>
        step({
          matches: new Uint32Array([5, 2, 3, 8, 0, 3]),
          reset: true,
          complete: true,
          cursor: undefined
        })
      )
    }
    const r = runToResult(engine) as FederatedPaneScanResult
    expect(r.matches.every((m) => m.snippet === null)).toBe(true)
  })

  it('settles null (never throws) when the engine is freed mid-run', () => {
    const engine: FederatedScanEngine = {
      searchBudgeted: vi.fn(() => {
        throw new Error('null pointer passed to rust')
      })
    }
    expect(runToResult(engine)).toBeNull()
  })
})
