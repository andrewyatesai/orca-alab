import { describe, expect, it, vi } from 'vitest'
import type { OrcaRuntimeService } from '../../orca-runtime'
import { isStreamingMethod } from '../core'
import { TERMINAL_METHODS } from './terminal'

describe('terminal.multiplex pre-aborted entry guard', () => {
  it('registers nothing and resolves when dispatched on an already-closed socket', async () => {
    const method = TERMINAL_METHODS.find((candidate) => candidate.name === 'terminal.multiplex')
    if (!method || !isStreamingMethod(method)) {
      throw new Error('terminal.multiplex must be a streaming method')
    }

    // Why: a frame dispatched from a queued batch after the socket closed enters
    // with an already-aborted signal. Without the entry guard the handler would
    // re-register the control binary handler + subscription cleanup for a dead
    // connection (never swept) and await a promise that never settles.
    const registerSubscriptionCleanup = vi.fn()
    const runtime = { registerSubscriptionCleanup } as unknown as OrcaRuntimeService
    const registerBinaryStreamHandler = vi.fn(() => vi.fn())
    const sendBinary = vi.fn(() => true)
    const controller = new AbortController()
    controller.abort()
    const emit = vi.fn()

    await expect(
      method.handler(
        {},
        {
          runtime,
          connectionId: 'c1',
          sendBinary,
          registerBinaryStreamHandler,
          signal: controller.signal
        },
        emit
      )
    ).resolves.toBeUndefined()

    expect(registerBinaryStreamHandler).not.toHaveBeenCalled()
    expect(registerSubscriptionCleanup).not.toHaveBeenCalled()
    expect(emit).not.toHaveBeenCalled()
  })
})
