import { describe, expect, it } from 'vitest'
import type { DaemonRuntimeStatus } from '../../../../preload/api-types'
import { getDaemonStatusIndicatorCopy, getDaemonStatusToastCopy } from './daemon-status-copy'

function makeStatus(overrides: Partial<DaemonRuntimeStatus>): DaemonRuntimeStatus {
  return { state: 'running', cause: null, detail: null, updatedAt: 1, ...overrides }
}

describe('getDaemonStatusToastCopy', () => {
  it('returns null for states that never toast', () => {
    expect(getDaemonStatusToastCopy(makeStatus({ state: 'running' }))).toBeNull()
    expect(getDaemonStatusToastCopy(makeStatus({ state: 'starting' }))).toBeNull()
  })

  it('offers a plain Retry for a failed launch', () => {
    const copy = getDaemonStatusToastCopy(
      makeStatus({ state: 'failed', cause: 'launch-failed', detail: 'no binary' })
    )
    expect(copy).not.toBeNull()
    expect(copy?.title).toContain('unavailable')
    expect(copy?.actionLabel).toBe('Retry')
  })

  it('warns that restarting closes panes when a degraded daemon still owns sessions', () => {
    const copy = getDaemonStatusToastCopy(
      makeStatus({ state: 'degraded-fallback', cause: 'spawn-unhealthy' })
    )
    expect(copy?.actionLabel).toBe('Restart daemon')
    expect(copy?.description).toContain('closes open terminal panes')
  })

  it('uses timeout copy (still warning about panes) for the startup fail-open case', () => {
    const copy = getDaemonStatusToastCopy(
      makeStatus({ state: 'degraded-fallback', cause: 'startup-timeout' })
    )
    expect(copy?.actionLabel).toBe('Retry')
    expect(copy?.description).toContain('closes open terminal panes')
  })
})

describe('getDaemonStatusIndicatorCopy', () => {
  it('hides the indicator while running or starting', () => {
    expect(getDaemonStatusIndicatorCopy(makeStatus({ state: 'running' }))).toBeNull()
    expect(getDaemonStatusIndicatorCopy(makeStatus({ state: 'starting' }))).toBeNull()
  })

  it('labels failed and degraded states and points at Manage Sessions', () => {
    for (const state of ['failed', 'degraded-fallback'] as const) {
      const copy = getDaemonStatusIndicatorCopy(makeStatus({ state }))
      expect(copy?.label).toBe('Terminal persistence off')
      expect(copy?.tooltip).toContain('Manage Sessions')
      expect(copy?.ariaLabel.length).toBeGreaterThan(0)
    }
  })
})
