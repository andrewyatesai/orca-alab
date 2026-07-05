// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DaemonRuntimeStatus } from '../../../preload/api-types'
import {
  getDaemonRuntimeStatusSnapshot,
  resetDaemonRuntimeStatusStoreForTest,
  subscribeDaemonRuntimeStatus
} from './daemon-runtime-status-store'

function makeStatus(overrides: Partial<DaemonRuntimeStatus>): DaemonRuntimeStatus {
  return { state: 'running', cause: null, detail: null, updatedAt: 1, ...overrides }
}

describe('daemon-runtime-status-store', () => {
  let emitStatus: ((status: DaemonRuntimeStatus) => void) | null = null
  let resolveGet: ((status: DaemonRuntimeStatus) => void) | null = null

  beforeEach(() => {
    resetDaemonRuntimeStatusStoreForTest()
    ;(window as unknown as { api: unknown }).api = {
      daemonStatus: {
        get: vi.fn(
          () =>
            new Promise<DaemonRuntimeStatus>((resolve) => {
              resolveGet = resolve
            })
        ),
        relaunch: vi.fn(),
        onChanged: (callback: (status: DaemonRuntimeStatus) => void) => {
          emitStatus = callback
          return () => {
            emitStatus = null
          }
        }
      }
    }
  })

  afterEach(() => {
    resetDaemonRuntimeStatusStoreForTest()
    emitStatus = null
    resolveGet = null
  })

  it('starts from the placeholder starting status', () => {
    expect(getDaemonRuntimeStatusSnapshot()).toMatchObject({ state: 'starting', updatedAt: 0 })
  })

  it('applies change events and notifies subscribers', () => {
    const listener = vi.fn()
    subscribeDaemonRuntimeStatus(listener)

    emitStatus?.(makeStatus({ state: 'failed', cause: 'launch-failed', updatedAt: 5 }))

    expect(listener).toHaveBeenCalledTimes(1)
    expect(getDaemonRuntimeStatusSnapshot().state).toBe('failed')
  })

  it('ignores a stale get() reply that lands after a fresher change event', async () => {
    subscribeDaemonRuntimeStatus(vi.fn())

    emitStatus?.(makeStatus({ state: 'failed', cause: 'launch-failed', updatedAt: 10 }))
    resolveGet?.(makeStatus({ state: 'starting', updatedAt: 4 }))
    await Promise.resolve()
    await Promise.resolve()

    expect(getDaemonRuntimeStatusSnapshot().state).toBe('failed')
  })

  it('accepts the get() reply when it is the freshest information', async () => {
    subscribeDaemonRuntimeStatus(vi.fn())

    resolveGet?.(makeStatus({ state: 'degraded-fallback', cause: 'spawn-unhealthy', updatedAt: 9 }))
    await Promise.resolve()
    await Promise.resolve()

    expect(getDaemonRuntimeStatusSnapshot().state).toBe('degraded-fallback')
  })

  it('unsubscribing stops notifications without tearing down the shared IPC subscription', () => {
    const listener = vi.fn()
    const unsubscribe = subscribeDaemonRuntimeStatus(listener)
    unsubscribe()

    emitStatus?.(makeStatus({ state: 'failed', updatedAt: 6 }))

    expect(listener).not.toHaveBeenCalled()
    // The module keeps tracking status for the next subscriber.
    expect(getDaemonRuntimeStatusSnapshot().state).toBe('failed')
  })
})
