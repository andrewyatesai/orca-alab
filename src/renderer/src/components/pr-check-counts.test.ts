import { describe, expect, it, vi } from 'vitest'

import { getCheckConclusion, getCheckCounts } from './pr-check-counts'
import type { PRCheckDetail } from '../../../shared/types'

function check(patch: Partial<PRCheckDetail>): PRCheckDetail {
  return {
    name: 'ci',
    status: 'completed',
    conclusion: 'success',
    url: null,
    ...patch
  }
}

describe('getCheckConclusion', () => {
  it('coalesces a null conclusion (queued/in_progress checks) to pending', () => {
    expect(getCheckConclusion(check({ status: 'in_progress', conclusion: null }))).toBe('pending')
  })
})

describe('getCheckCounts', () => {
  it('buckets every declared conclusion', () => {
    expect(
      getCheckCounts([
        check({ conclusion: 'success' }),
        check({ conclusion: 'failure' }),
        check({ conclusion: 'cancelled' }),
        check({ conclusion: 'timed_out' }),
        check({ conclusion: 'action_required' }),
        check({ conclusion: 'skipped' }),
        check({ conclusion: 'neutral' }),
        check({ status: 'queued', conclusion: null })
      ])
    ).toEqual({ passing: 1, failing: 3, needsAction: 1, pending: 1, skipped: 1, neutral: 1 })
  })

  it('fails closed on an out-of-union conclusion instead of dropping it from every bucket', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      // Why: conclusions cross IPC/relay as JSON — a version-skewed producer can send values outside the union.
      expect(
        getCheckCounts([
          check({ conclusion: 'success' }),
          check({ conclusion: 'startup_failure' as PRCheckDetail['conclusion'] })
        ])
      ).toEqual({ passing: 1, failing: 1, needsAction: 0, pending: 0, skipped: 0, neutral: 0 })
      expect(warn).toHaveBeenCalledTimes(1)
    } finally {
      warn.mockRestore()
    }
  })
})
