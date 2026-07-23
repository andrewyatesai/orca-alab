import { describe, expect, it } from 'vitest'
import { filterDepthExtensionMatches, planFederatedSessionDedup } from './federated-session-dedup'

describe('planFederatedSessionDedup', () => {
  it('excludes attached sessions from standalone daemon results (no double report)', () => {
    const plan = planFederatedSessionDedup(
      [
        { paneKey: 't1:a', sessionId: 's-live', oldestLiveRow: 400 },
        { paneKey: 't1:b', sessionId: null, oldestLiveRow: 0 }
      ],
      ['s-live', 's-dead']
    )
    expect(plan.standaloneSessionIds).toEqual(['s-dead'])
  })

  it('attached sessions become depth extensions with the live window cutoff', () => {
    const plan = planFederatedSessionDedup(
      [{ paneKey: 't1:a', sessionId: 's-live', oldestLiveRow: 400 }],
      ['s-live']
    )
    expect(plan.depthExtensions).toEqual([{ sessionId: 's-live', paneKey: 't1:a', cutoffRow: 400 }])
  })

  it('an attached session with an UNKNOWN live window gets no depth extension', () => {
    // Never risk double-reporting rows the live scan already covered.
    const plan = planFederatedSessionDedup(
      [{ paneKey: 't1:a', sessionId: 's-live', oldestLiveRow: null }],
      ['s-live']
    )
    expect(plan.depthExtensions).toEqual([])
    expect(plan.standaloneSessionIds).toEqual([])
  })

  it('dead sessions stay standalone', () => {
    const plan = planFederatedSessionDedup([], ['s-dead-1', 's-dead-2'])
    expect(plan.standaloneSessionIds).toEqual(['s-dead-1', 's-dead-2'])
    expect(plan.depthExtensions).toEqual([])
  })
})

describe('filterDepthExtensionMatches', () => {
  it('keeps only matches STRICTLY BELOW the cutoff row', () => {
    const filtered = filterDepthExtensionMatches(
      [
        { absRow: 399, col: 0, len: 3, snippet: null },
        { absRow: 400, col: 0, len: 3, snippet: null },
        { absRow: 500, col: 0, len: 3, snippet: null }
      ],
      400
    )
    expect(filtered.map((m) => m.absRow)).toEqual([399])
  })
})
