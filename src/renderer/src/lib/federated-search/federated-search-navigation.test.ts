import { describe, expect, it, vi } from 'vitest'
import {
  jumpToFederatedResult,
  nearestFederatedMatchRow,
  type FederatedJumpDeps,
  type FederatedJumpPane
} from './federated-search-navigation'
import type { FederatedMatch, FederatedPaneRef } from './federated-search-model'

const paneRef: FederatedPaneRef = {
  tabId: 't1',
  leafId: 'leaf',
  paneKey: 't1:leaf',
  worktreeId: 'wt',
  title: 'tab'
}

const match: FederatedMatch = { absRow: 120, col: 0, len: 3, snippet: null }
const opts = { caseSensitive: false, isRegex: false }

function jumpPane(): {
  scrollToLine: ReturnType<typeof vi.fn<(absRow: number) => void>>
  focus: ReturnType<typeof vi.fn<() => void>>
} & FederatedJumpPane {
  return { scrollToLine: vi.fn<(absRow: number) => void>(), focus: vi.fn<() => void>() }
}

function deps(overrides: Partial<FederatedJumpDeps>): FederatedJumpDeps {
  return {
    resolvePane: () => null,
    activatePane: vi.fn(),
    waitForPane: async () => null,
    rerunQueryInPane: async () => null,
    expandDaemonInline: () => false,
    notifyJumpOutcome: vi.fn(),
    ...overrides
  }
}

describe('nearestFederatedMatchRow', () => {
  it('picks the row nearest the recorded one (ties break newer)', () => {
    const rows = [{ absRow: 90 }, { absRow: 110 }, { absRow: 130 }]
    expect(nearestFederatedMatchRow(rows, 108)).toBe(110)
    expect(nearestFederatedMatchRow(rows, 120)).toBe(130)
    expect(nearestFederatedMatchRow([], 120)).toBeNull()
  })
})

describe('jumpToFederatedResult', () => {
  it('live pane: activates, focuses, and jumps to the exact absolute row', async () => {
    const pane = jumpPane()
    const activatePane = vi.fn()
    const outcome = await jumpToFederatedResult(
      { paneRef, sessionId: null },
      match,
      'foo',
      opts,
      deps({ resolvePane: () => pane, activatePane })
    )
    expect(outcome).toBe('jumped')
    expect(activatePane).toHaveBeenCalledWith(paneRef)
    expect(pane.focus).toHaveBeenCalled()
    expect(pane.scrollToLine).toHaveBeenCalledWith(120)
  })

  it('parked pane: un-parks, re-runs the pinned query, jumps NEAREST the recorded row', async () => {
    const pane = jumpPane()
    const rerunQueryInPane = vi.fn(async () => [{ absRow: 90 }, { absRow: 115 }])
    const outcome = await jumpToFederatedResult(
      { paneRef, sessionId: null },
      match,
      'foo',
      opts,
      deps({
        resolvePane: () => null, // parked → not mounted at click time
        waitForPane: async () => pane,
        rerunQueryInPane
      })
    )
    expect(outcome).toBe('jumped-nearest')
    expect(rerunQueryInPane).toHaveBeenCalledWith(paneRef, 'foo', opts)
    expect(pane.scrollToLine).toHaveBeenCalledWith(115)
  })

  it('parked pane whose restored buffer lost the match: toast, no guess-jump', async () => {
    const pane = jumpPane()
    const notifyJumpOutcome = vi.fn()
    const outcome = await jumpToFederatedResult(
      { paneRef, sessionId: null },
      match,
      'foo',
      opts,
      deps({
        waitForPane: async () => pane,
        rerunQueryInPane: async () => [],
        notifyJumpOutcome
      })
    )
    expect(outcome).toBe('missing-match')
    expect(notifyJumpOutcome).toHaveBeenCalledWith('missing-match')
    expect(pane.scrollToLine).not.toHaveBeenCalled()
  })

  it('stale paneRef with a persisted session degrades to daemon inline expansion', async () => {
    const expandDaemonInline = vi.fn(() => true)
    const outcome = await jumpToFederatedResult(
      { paneRef, sessionId: 's-live' },
      match,
      'foo',
      opts,
      deps({ expandDaemonInline }) // resolve fails AND waitForPane times out
    )
    expect(outcome).toBe('daemon-inline')
    expect(expandDaemonInline).toHaveBeenCalledWith('s-live', 120)
  })

  it('stale paneRef without a session: "pane no longer available" toast, never a throw', async () => {
    const notifyJumpOutcome = vi.fn()
    const outcome = await jumpToFederatedResult(
      { paneRef, sessionId: null },
      match,
      'foo',
      opts,
      deps({ notifyJumpOutcome })
    )
    expect(outcome).toBe('pane-unavailable')
    expect(notifyJumpOutcome).toHaveBeenCalledWith('pane-unavailable')
  })

  it('dead daemon session (no paneRef) goes straight to inline expansion', async () => {
    const expandDaemonInline = vi.fn(() => true)
    const outcome = await jumpToFederatedResult(
      { paneRef: undefined, sessionId: 's-dead' },
      match,
      'foo',
      opts,
      deps({ expandDaemonInline })
    )
    expect(outcome).toBe('daemon-inline')
  })
})
