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
