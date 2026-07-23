import { describe, expect, it } from 'vitest'
import {
  federatedGroupKey,
  mergeFederatedBatch,
  orderFederatedGroups,
  type FederatedResultGroup
} from './federated-search-grouping'
import {
  FEDERATED_TOP_K_MATCHES,
  type FederatedMatch,
  type FederatedPaneBatch,
  type FederatedPaneRef
} from './federated-search-model'

function paneRef(key: string): FederatedPaneRef {
  const [tabId, leafId] = key.split(':')
  return { tabId, leafId, paneKey: key, worktreeId: 'wt', title: 'tab' }
}

function match(absRow: number): FederatedMatch {
  return { absRow, col: 0, len: 3, snippet: null }
}

function liveBatch(overrides: Partial<FederatedPaneBatch>): FederatedPaneBatch {
  return {
    paneRef: paneRef('t1:a'),
    sessionId: null,
    source: 'live',
    matches: [match(5), match(9)],
    total: 2,
    incomplete: false,
    approxTime: null,
    ...overrides
  }
}

describe('federatedGroupKey', () => {
  it('is source-keyed: sessionId first, pane fallback', () => {
    expect(federatedGroupKey({ sessionId: 's1', paneRef: paneRef('t1:a') })).toBe('session:s1')
    expect(federatedGroupKey({ sessionId: null, paneRef: paneRef('t1:a') })).toBe('pane:t1:a')
  })
})

describe('mergeFederatedBatch', () => {
  it('a daemon-backed live pane and its depth extension form ONE group', () => {
    const groups = new Map<string, FederatedResultGroup>()
    mergeFederatedBatch(groups, liveBatch({ sessionId: 's1', matches: [match(500), match(900)] }))
    mergeFederatedBatch(
      groups,
      liveBatch({
        sessionId: 's1',
        source: 'daemon-history',
        depthExtension: true,
        matches: [match(100), match(200)],
        total: 2
      }),
      400
    )
    expect(groups.size).toBe(1)
    const group = [...groups.values()][0]
    expect(group.hasDepthExtension).toBe(true)
    // Live pane stays the group's face.
    expect(group.source).toBe('live')
    expect(group.paneRef?.paneKey).toBe('t1:a')
    // Newest-first across live + depth rows.
    expect(group.matches.map((m) => m.absRow)).toEqual([900, 500, 200, 100])
  })

  it('depth-extension matches at/above the cutoff are dropped (defense in depth)', () => {
    const groups = new Map<string, FederatedResultGroup>()
    mergeFederatedBatch(groups, liveBatch({ sessionId: 's1', matches: [match(500)] }))
    mergeFederatedBatch(
      groups,
      liveBatch({
        sessionId: 's1',
        depthExtension: true,
        matches: [match(100), match(450), match(500)],
        total: 3
      }),
      400
    )
    const group = [...groups.values()][0]
    expect(group.matches.map((m) => m.absRow)).toEqual([500, 100])
  })

  it('a depth-extension batch with NO known cutoff fails closed (drops all matches)', () => {
    const groups = new Map<string, FederatedResultGroup>()
    mergeFederatedBatch(groups, liveBatch({ sessionId: 's1', matches: [match(500)], total: 1 }))
    // A skewed adapter flags depth extension but the controller resolves no
    // cutoff for the session: accepting the batch could double-report live rows.
    mergeFederatedBatch(
      groups,
      liveBatch({
        sessionId: 's1',
        depthExtension: true,
        matches: [match(100), match(500)],
        total: 2
      })
      // cutoffRow deliberately undefined
    )
    const group = [...groups.values()][0]
    expect(group.matches.map((m) => m.absRow)).toEqual([500])
    expect(group.total).toBe(1)
  })

  it('orders matches newest-first within a group and caps at top-K with honest total', () => {
    const groups = new Map<string, FederatedResultGroup>()
    const many = Array.from({ length: FEDERATED_TOP_K_MATCHES + 10 }, (_, i) => match(i))
    mergeFederatedBatch(groups, liveBatch({ matches: many, total: many.length }))
    const group = [...groups.values()][0]
    expect(group.matches).toHaveLength(FEDERATED_TOP_K_MATCHES)
    expect(group.matches[0].absRow).toBe(FEDERATED_TOP_K_MATCHES + 9)
    expect(group.total).toBe(FEDERATED_TOP_K_MATCHES + 10)
  })

  it('a dead-session batch (no paneRef) forms a standalone daemon group', () => {
    const groups = new Map<string, FederatedResultGroup>()
    mergeFederatedBatch(
      groups,
      liveBatch({ paneRef: undefined, sessionId: 's-dead', source: 'daemon-history' })
    )
    const group = [...groups.values()][0]
    expect(group.key).toBe('session:s-dead')
    expect(group.paneRef).toBeUndefined()
    expect(group.source).toBe('daemon-history')
  })

  it('(d) totals are NOT summed across same-session batches (re-emit REPLACES)', () => {
    const groups = new Map<string, FederatedResultGroup>()
    // A live pane's batch reports total 5 for session s1.
    mergeFederatedBatch(groups, liveBatch({ sessionId: 's1', matches: [match(9)], total: 5 }))
    // The SAME source re-emits (incremental re-rank / streaming update) with an
    // updated total 7 — the group total must REPLACE, not become 5 + 7 = 12.
    mergeFederatedBatch(groups, liveBatch({ sessionId: 's1', matches: [match(9)], total: 7 }))
    const group = [...groups.values()][0]
    expect(group.total).toBe(7)
  })

  it('(d) a live pane and its daemon depth extension sum ONCE each (source once + disjoint depth)', () => {
    const groups = new Map<string, FederatedResultGroup>()
    // Live source total 10 (its window), depth extension adds 2 disjoint rows.
    mergeFederatedBatch(groups, liveBatch({ sessionId: 's1', matches: [match(900)], total: 10 }))
    mergeFederatedBatch(
      groups,
      liveBatch({
        sessionId: 's1',
        source: 'daemon-history',
        depthExtension: true,
        matches: [match(100), match(200)],
        total: 999 // the daemon's own (possibly-overlapping) total is IGNORED
      }),
      400
    )
    const group = [...groups.values()][0]
    // 10 (live, counted once) + 2 (disjoint depth rows) — NOT 10 + 999.
    expect(group.total).toBe(12)
  })

  it('(d) a same-session live + remote batch count each source ONCE, not summed twice', () => {
    const groups = new Map<string, FederatedResultGroup>()
    mergeFederatedBatch(groups, liveBatch({ sessionId: 's1', matches: [match(9)], total: 4 }))
    // A remote batch that resolves the SAME session re-reports 4 (the same
    // authority) — merging must not double it to 8.
    mergeFederatedBatch(
      groups,
      liveBatch({ sessionId: 's1', source: 'remote', matches: [match(9)], total: 4 })
    )
    const group = [...groups.values()][0]
    // Both primaries describe the same session window: the latest total REPLACES,
    // it is never summed to 8, and the identical span is deduped in matches.
    expect(group.total).toBe(4)
    expect(group.matches).toHaveLength(1)
  })

  it('propagates incomplete and over-budget honesty flags', () => {
    const groups = new Map<string, FederatedResultGroup>()
    mergeFederatedBatch(groups, liveBatch({ incomplete: true }))
    mergeFederatedBatch(groups, liveBatch({ matches: [], total: 0, degraded: 'over-budget' }))
    const group = [...groups.values()][0]
    expect(group.incomplete).toBe(true)
    expect(group.overBudget).toBe(true)
  })
})

describe('orderFederatedGroups', () => {
  it('orders focused → visible → recency → dead daemon sessions', () => {
    const groups = new Map<string, FederatedResultGroup>()
    mergeFederatedBatch(groups, liveBatch({ paneRef: paneRef('t1:focused') }))
    mergeFederatedBatch(groups, liveBatch({ paneRef: paneRef('t1:visible') }))
    mergeFederatedBatch(groups, liveBatch({ paneRef: paneRef('t2:recent'), source: 'hidden' }))
    mergeFederatedBatch(groups, liveBatch({ paneRef: paneRef('t2:old'), source: 'hidden' }))
    mergeFederatedBatch(
      groups,
      liveBatch({ paneRef: undefined, sessionId: 's-dead', source: 'daemon-history' })
    )
    const ordered = orderFederatedGroups(groups.values(), {
      focusedPaneKey: 't1:focused',
      visiblePaneKeys: new Set(['t1:focused', 't1:visible']),
      outputRecency: (paneKey) => (paneKey === 't2:recent' ? 100 : 1)
    })
    expect(ordered.map((g) => g.paneRef?.paneKey ?? g.key)).toEqual([
      't1:focused',
      't1:visible',
      't2:recent',
      't2:old',
      'session:s-dead'
    ])
  })
})
