import { describe, expect, it, vi } from 'vitest'
import type { OrcaRuntimeService } from '../../orca-runtime'
import { isStreamingMethod } from '../core'
import { NOTIFICATION_METHODS } from './notifications'

function method(name: string) {
  const found = NOTIFICATION_METHODS.find((candidate) => candidate.name === name)
  if (!found) {
    throw new Error(`Missing method ${name}`)
  }
  return found
}

describe('notification RPC methods', () => {
  it('refuses notifications.unsubscribe for a subscription owned by another connection', async () => {
    const cleanupSubscription = vi.fn()
    const runtime = { cleanupSubscription } as unknown as OrcaRuntimeService
    const unsubscribe = method('notifications.unsubscribe')
    if (isStreamingMethod(unsubscribe)) {
      throw new Error('notifications.unsubscribe must be a request method')
    }

    // Connection B must not tear down connection A's push channel by id.
    const foreign = await unsubscribe.handler(
      { subscriptionId: 'notifications-connA-4' },
      { runtime, connectionId: 'connB' }
    )
    expect(foreign).toEqual({ unsubscribed: false })
    expect(cleanupSubscription).not.toHaveBeenCalled()

    // The owning connection can still tear its own subscription down.
    const own = await unsubscribe.handler(
      { subscriptionId: 'notifications-connA-4' },
      { runtime, connectionId: 'connA' }
    )
    expect(own).toEqual({ unsubscribed: true })
    expect(cleanupSubscription).toHaveBeenCalledWith('notifications-connA-4')
  })
})
