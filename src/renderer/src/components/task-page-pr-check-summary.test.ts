import { describe, expect, it, vi } from 'vitest'

import { deriveTaskPagePRCheckSummary } from './task-page-pr-check-summary'
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

describe('deriveTaskPagePRCheckSummary', () => {
  it('returns a none summary for PRs with no checks', () => {
    expect(deriveTaskPagePRCheckSummary([])).toEqual({
      state: 'none',
      total: 0,
      passed: 0,
      failed: 0,
      pending: 0
    })
  })

  it('counts failing checks before pending and passing checks', () => {
    expect(
      deriveTaskPagePRCheckSummary([
        check({ conclusion: 'success' }),
        check({ conclusion: 'failure' }),
        check({ status: 'in_progress', conclusion: null })
      ])
    ).toEqual({
      state: 'failure',
      total: 3,
      passed: 1,
      failed: 1,
      pending: 1
    })
  })

  it('treats neutral and skipped checks as passed for the compact PR table label', () => {
    expect(
      deriveTaskPagePRCheckSummary([
        check({ conclusion: 'success' }),
        check({ conclusion: 'neutral' }),
        check({ conclusion: 'skipped' })
      ])
    ).toEqual({
      state: 'success',
      total: 3,
      passed: 3,
      failed: 0,
      pending: 0
    })
  })

  it('counts timed_out and cancelled as failed, never as passed', () => {
    expect(
      deriveTaskPagePRCheckSummary([
        check({ conclusion: 'timed_out' }),
        check({ conclusion: 'cancelled' })
      ])
    ).toEqual({
      state: 'failure',
      total: 2,
      passed: 0,
      failed: 2,
      pending: 0
    })
  })

  it('counts an explicit pending conclusion as pending even when status is completed', () => {
    expect(
      deriveTaskPagePRCheckSummary([
        check({ conclusion: 'success' }),
        check({ status: 'completed', conclusion: 'pending' })
      ])
    ).toEqual({
      state: 'pending',
      total: 2,
      passed: 1,
      failed: 0,
      pending: 1
    })
  })

  it('fails closed on an out-of-union conclusion instead of reading green', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      // Why: conclusions cross IPC/relay as JSON — a version-skewed producer can send values outside the union.
      expect(
        deriveTaskPagePRCheckSummary([
          check({ conclusion: 'success' }),
          check({ conclusion: 'stale' as PRCheckDetail['conclusion'] })
        ])
      ).toEqual({
        state: 'failure',
        total: 2,
        passed: 1,
        failed: 1,
        pending: 0
      })
      expect(warn).toHaveBeenCalledTimes(1)
    } finally {
      warn.mockRestore()
    }
  })

  it('counts action_required as failed so a blocked PR never reads as passing', () => {
    expect(
      deriveTaskPagePRCheckSummary([
        check({ conclusion: 'success' }),
        check({ conclusion: 'action_required' })
      ])
    ).toEqual({
      state: 'failure',
      total: 2,
      passed: 1,
      failed: 1,
      pending: 0
    })
  })
})
