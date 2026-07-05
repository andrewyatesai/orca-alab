import { describe, expect, it } from 'vitest'
import {
  isDaemonUnavailableState,
  nextDaemonStatusToastAction
} from './daemon-status-toast-transitions'

describe('nextDaemonStatusToastAction', () => {
  it('shows the sticky toast on entering failed or degraded-fallback', () => {
    expect(nextDaemonStatusToastAction({ stickyShown: false, state: 'failed' })).toBe(
      'show-sticky'
    )
    expect(nextDaemonStatusToastAction({ stickyShown: false, state: 'degraded-fallback' })).toBe(
      'show-sticky'
    )
  })

  it('keeps updating the sticky toast in place while the state stays unavailable', () => {
    // e.g. failed → degraded-fallback after a partially-successful retry: the
    // stable toast id means "show" replaces rather than stacks.
    expect(nextDaemonStatusToastAction({ stickyShown: true, state: 'degraded-fallback' })).toBe(
      'show-sticky'
    )
    expect(nextDaemonStatusToastAction({ stickyShown: true, state: 'failed' })).toBe('show-sticky')
  })

  it('dismisses and celebrates when the daemon recovers to running', () => {
    expect(nextDaemonStatusToastAction({ stickyShown: true, state: 'running' })).toBe(
      'dismiss-and-celebrate'
    )
  })

  it('dismisses without celebrating while a retry is starting', () => {
    expect(nextDaemonStatusToastAction({ stickyShown: true, state: 'starting' })).toBe('dismiss')
  })

  it('does nothing on healthy states when no sticky toast is up', () => {
    expect(nextDaemonStatusToastAction({ stickyShown: false, state: 'running' })).toBe('none')
    expect(nextDaemonStatusToastAction({ stickyShown: false, state: 'starting' })).toBe('none')
  })
})

describe('isDaemonUnavailableState', () => {
  it('flags only failed and degraded-fallback', () => {
    expect(isDaemonUnavailableState('failed')).toBe(true)
    expect(isDaemonUnavailableState('degraded-fallback')).toBe(true)
    expect(isDaemonUnavailableState('running')).toBe(false)
    expect(isDaemonUnavailableState('starting')).toBe(false)
  })
})
