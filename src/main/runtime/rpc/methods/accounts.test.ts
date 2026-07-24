import { describe, expect, it, vi } from 'vitest'
import type { OrcaRuntimeService } from '../../orca-runtime'
import { isStreamingMethod } from '../core'
import { ACCOUNT_METHODS } from './accounts'

function method(name: string) {
  const found = ACCOUNT_METHODS.find((candidate) => candidate.name === name)
  if (!found) {
    throw new Error(`Missing method ${name}`)
  }
  return found
}

describe('account RPC methods', () => {
  it('keeps explicit account-list refreshes on the forced refresh lane', async () => {
    const snapshot = { claude: null, codex: null }
    const runtime = {
      refreshAccountsForMobile: vi.fn().mockResolvedValue(undefined),
      getAccountsSnapshot: vi.fn(() => snapshot)
    } as unknown as OrcaRuntimeService
    const list = method('accounts.list')
    if (isStreamingMethod(list)) {
      throw new Error('accounts.list must be a request method')
    }

    await expect(list.handler(undefined, { runtime })).resolves.toBe(snapshot)
    expect(runtime.refreshAccountsForMobile).toHaveBeenCalledOnce()
  })

  it('uses a stale-aware refresh when a connection replays the subscription', async () => {
    const snapshot = { claude: null, codex: null }
    let cleanup: (() => void) | undefined
    const runtime = {
      getAccountsSnapshot: vi.fn(() => snapshot),
      onAccountsChanged: vi.fn(() => vi.fn()),
      registerSubscriptionCleanup: vi.fn((_id: string, nextCleanup: () => void) => {
        cleanup = nextCleanup
      }),
      refreshAccountsForMobile: vi.fn().mockResolvedValue(undefined),
      refreshAccountsForMobileSubscriber: vi.fn().mockResolvedValue(undefined)
    } as unknown as OrcaRuntimeService
    const subscribe = method('accounts.subscribe')
    if (!isStreamingMethod(subscribe)) {
      throw new Error('accounts.subscribe must be a streaming method')
    }
    const emit = vi.fn()

    const running = subscribe.handler(undefined, { runtime, connectionId: 'connection-1' }, emit)
    await vi.waitFor(() => {
      expect(runtime.refreshAccountsForMobileSubscriber).toHaveBeenCalledOnce()
    })

    expect(runtime.refreshAccountsForMobile).not.toHaveBeenCalled()
    expect(emit).toHaveBeenCalledWith(expect.objectContaining({ type: 'ready', snapshot }))
    cleanup?.()
    await running
  })

  it('refuses accounts.unsubscribe for a subscription owned by another connection', async () => {
    const cleanupSubscription = vi.fn()
    const runtime = { cleanupSubscription } as unknown as OrcaRuntimeService
    const unsubscribe = method('accounts.unsubscribe')
    if (isStreamingMethod(unsubscribe)) {
      throw new Error('accounts.unsubscribe must be a request method')
    }

    // Connection B must not tear down connection A's account stream by id.
    const foreign = await unsubscribe.handler(
      { subscriptionId: 'accounts-connA-2' },
      { runtime, connectionId: 'connB' }
    )
    expect(foreign).toEqual({ unsubscribed: false })
    expect(cleanupSubscription).not.toHaveBeenCalled()

    // The owning connection can still tear its own subscription down.
    const own = await unsubscribe.handler(
      { subscriptionId: 'accounts-connA-2' },
      { runtime, connectionId: 'connA' }
    )
    expect(own).toEqual({ unsubscribed: true })
    expect(cleanupSubscription).toHaveBeenCalledWith('accounts-connA-2')
  })
})
