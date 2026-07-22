import { afterEach, describe, expect, it, vi } from 'vitest'
import { isCheckJobFailureState } from './check-run-failure-conclusions'

describe('isCheckJobFailureState', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it.each([
    'failure',
    'failed',
    'action_required',
    'cancelled',
    'stale',
    'startup_failure',
    'timed_out'
  ])('classifies raw job conclusion %s as failure', (state) => {
    expect(isCheckJobFailureState(state)).toBe(true)
  })

  it.each([
    'success',
    'neutral',
    'skipped',
    'pending',
    'queued',
    'in_progress',
    'waiting',
    'requested',
    'completed',
    null,
    undefined
  ])('classifies %s as non-failure without warning', (state) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    expect(isCheckJobFailureState(state)).toBe(false)
    expect(warn).not.toHaveBeenCalled()
  })

  it('degrades unknown values to non-failure and warns once per value', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    expect(isCheckJobFailureState('some_future_conclusion')).toBe(false)
    expect(isCheckJobFailureState('some_future_conclusion')).toBe(false)
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn).toHaveBeenCalledWith(
      '[checks] Unknown check job conclusion/status: some_future_conclusion'
    )
  })
})
