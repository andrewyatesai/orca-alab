import { afterEach, describe, expect, it, vi } from 'vitest'
import { buildGitHubCheckSummary } from './github-check-summary'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('buildGitHubCheckSummary', () => {
  it('returns none for empty check lists', () => {
    expect(buildGitHubCheckSummary([])).toEqual({
      state: 'none',
      total: 0,
      passed: 0,
      failed: 0,
      pending: 0
    })
  })

  it('prioritizes failed checks over pending checks', () => {
    expect(
      buildGitHubCheckSummary([
        { status: 'completed', conclusion: 'success' },
        { status: 'queued', conclusion: null },
        { status: 'completed', conclusion: 'timed_out' }
      ])
    ).toEqual({
      state: 'failure',
      total: 3,
      passed: 1,
      failed: 1,
      pending: 1
    })
  })

  it('marks all completed non-failing checks as successful', () => {
    expect(
      buildGitHubCheckSummary([
        { status: 'completed', conclusion: 'success' },
        { status: 'completed', conclusion: 'neutral' },
        { status: 'completed', conclusion: 'skipped' }
      ])
    ).toEqual({
      state: 'success',
      total: 3,
      passed: 3,
      failed: 0,
      pending: 0
    })
  })

  it('counts action_required as failed so a merge-blocked PR never reads green', () => {
    expect(
      buildGitHubCheckSummary([
        { status: 'completed', conclusion: 'success' },
        { status: 'completed', conclusion: 'action_required' }
      ])
    ).toEqual({
      state: 'failure',
      total: 2,
      passed: 1,
      failed: 1,
      pending: 0
    })
  })

  it('counts every failure-class conclusion as failed', () => {
    expect(
      buildGitHubCheckSummary([
        { status: 'completed', conclusion: 'failure' },
        { status: 'completed', conclusion: 'timed_out' },
        { status: 'completed', conclusion: 'cancelled' },
        { status: 'completed', conclusion: 'action_required' }
      ])
    ).toEqual({
      state: 'failure',
      total: 4,
      passed: 0,
      failed: 4,
      pending: 0
    })
  })

  it('treats a completed check with null conclusion as pending, not passed', () => {
    expect(
      buildGitHubCheckSummary([
        { status: 'completed', conclusion: null },
        { status: 'completed', conclusion: 'pending' }
      ])
    ).toEqual({
      state: 'pending',
      total: 2,
      passed: 0,
      failed: 0,
      pending: 2
    })
  })

  it('fails closed on unknown completed conclusions and warns', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    expect(buildGitHubCheckSummary([{ status: 'completed', conclusion: 'stale' }])).toEqual({
      state: 'failure',
      total: 1,
      passed: 0,
      failed: 1,
      pending: 0
    })
    expect(warn).toHaveBeenCalledWith('[github-check-summary] unknown check conclusion', {
      conclusion: 'stale'
    })
  })

  it('keeps unknown conclusions pending while the check has not completed', () => {
    expect(buildGitHubCheckSummary([{ status: 'queued', conclusion: 'stale' }])).toEqual({
      state: 'pending',
      total: 1,
      passed: 0,
      failed: 0,
      pending: 1
    })
  })
})
