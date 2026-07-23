import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  getPendingRunCommandConsent,
  requestRunCommandConsent,
  resetRunCommandConsentForTest,
  runOrDeferDeepLinkNavigation,
  settleRunCommandConsent,
  subscribeRunCommandConsent
} from './deep-link-consent-gate'
import type { DeepLinkRunConsentRequest } from './deep-link-consent-gate'

const runRequest = (command = 'echo hi'): DeepLinkRunConsentRequest => ({
  link: { kind: 'run', worktreeId: 'repo::wt', command },
  origin: { source: 'os' }
})

beforeEach(() => {
  resetRunCommandConsentForTest()
})

describe('requestRunCommandConsent', () => {
  it('stores the pending request and notifies subscribers', () => {
    const listener = vi.fn()
    subscribeRunCommandConsent(listener)

    expect(requestRunCommandConsent(runRequest())).toBe(true)

    expect(getPendingRunCommandConsent()?.link.command).toBe('echo hi')
    expect(listener).toHaveBeenCalledTimes(1)
  })

  it('drops a second request while one is open — never swaps dialog content', () => {
    requestRunCommandConsent(runRequest('first'))

    expect(requestRunCommandConsent(runRequest('second'))).toBe(false)

    expect(getPendingRunCommandConsent()?.link.command).toBe('first')
  })

  it('accepts a new request after the previous one settles', () => {
    requestRunCommandConsent(runRequest('first'))
    settleRunCommandConsent()

    expect(requestRunCommandConsent(runRequest('second'))).toBe(true)
    expect(getPendingRunCommandConsent()?.link.command).toBe('second')
  })
})

describe('runOrDeferDeepLinkNavigation', () => {
  it('runs navigation immediately when no consent dialog is open', () => {
    const navigate = vi.fn()

    runOrDeferDeepLinkNavigation(navigate)

    expect(navigate).toHaveBeenCalledTimes(1)
  })

  it('holds navigation while consent is open and flushes in order on settle', () => {
    const order: string[] = []
    requestRunCommandConsent(runRequest())

    runOrDeferDeepLinkNavigation(() => order.push('a'))
    runOrDeferDeepLinkNavigation(() => order.push('b'))
    expect(order).toEqual([])

    settleRunCommandConsent()

    expect(order).toEqual(['a', 'b'])
  })

  it('flushed navigations observe the dialog already closed', () => {
    let pendingDuringFlush: unknown = 'unset'
    requestRunCommandConsent(runRequest())
    runOrDeferDeepLinkNavigation(() => {
      pendingDuringFlush = getPendingRunCommandConsent()
    })

    settleRunCommandConsent()

    expect(pendingDuringFlush).toBeNull()
  })

  it('bounds the deferred queue at 4, dropping oldest entries', () => {
    const order: string[] = []
    requestRunCommandConsent(runRequest())

    for (const id of ['a', 'b', 'c', 'd', 'e', 'f']) {
      runOrDeferDeepLinkNavigation(() => order.push(id))
    }
    settleRunCommandConsent()

    expect(order).toEqual(['c', 'd', 'e', 'f'])
  })

  it('a navigation queued during flush is not lost (runs immediately post-settle)', () => {
    const order: string[] = []
    requestRunCommandConsent(runRequest())
    runOrDeferDeepLinkNavigation(() => {
      order.push('outer')
      runOrDeferDeepLinkNavigation(() => order.push('inner'))
    })

    settleRunCommandConsent()

    expect(order).toEqual(['outer', 'inner'])
  })
})

describe('settleRunCommandConsent', () => {
  it('is a no-op with no pending request', () => {
    const listener = vi.fn()
    subscribeRunCommandConsent(listener)

    settleRunCommandConsent()

    expect(listener).not.toHaveBeenCalled()
  })
})
