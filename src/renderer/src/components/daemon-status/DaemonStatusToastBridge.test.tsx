// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DaemonRuntimeStatus } from '../../../../preload/api-types'
import { resetDaemonRuntimeStatusStoreForTest } from '@/lib/daemon-runtime-status-store'

const { toastMocks } = vi.hoisted(() => ({
  toastMocks: {
    error: vi.fn(),
    success: vi.fn(),
    dismiss: vi.fn()
  }
}))

vi.mock('sonner', () => ({
  toast: {
    error: toastMocks.error,
    success: toastMocks.success,
    dismiss: toastMocks.dismiss
  }
}))

import { DAEMON_STATUS_TOAST_ID, DaemonStatusToastBridge } from './DaemonStatusToastBridge'

function makeStatus(overrides: Partial<DaemonRuntimeStatus>): DaemonRuntimeStatus {
  return { state: 'running', cause: null, detail: null, updatedAt: 1, ...overrides }
}

describe('DaemonStatusToastBridge', () => {
  let root: Root | null = null
  let container: HTMLElement | null = null
  let emitStatus: ((status: DaemonRuntimeStatus) => void) | null = null
  const relaunchMock = vi.fn(async () => ({ success: true }) as { success: boolean; error?: string })
  const getMock = vi.fn(async () => makeStatus({ state: 'starting', updatedAt: 0 }))

  beforeEach(() => {
    resetDaemonRuntimeStatusStoreForTest()
    toastMocks.error.mockClear()
    toastMocks.success.mockClear()
    toastMocks.dismiss.mockClear()
    relaunchMock.mockClear()
    relaunchMock.mockResolvedValue({ success: true })
    getMock.mockResolvedValue(makeStatus({ state: 'starting', updatedAt: 0 }))
    ;(window as unknown as { api: unknown }).api = {
      daemonStatus: {
        get: getMock,
        relaunch: relaunchMock,
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
    act(() => {
      root?.unmount()
    })
    root = null
    container?.remove()
    container = null
    resetDaemonRuntimeStatusStoreForTest()
  })

  async function mountBridge(): Promise<void> {
    container = document.createElement('div')
    document.body.appendChild(container)
    await act(async () => {
      root = createRoot(container as HTMLElement)
      root.render(createElement(DaemonStatusToastBridge))
    })
  }

  async function emit(status: DaemonRuntimeStatus): Promise<void> {
    await act(async () => {
      emitStatus?.(status)
    })
  }

  it('shows a sticky, dismissible toast with a retry action on entering failed', async () => {
    await mountBridge()
    await emit(makeStatus({ state: 'failed', cause: 'launch-failed', updatedAt: 2 }))

    expect(toastMocks.error).toHaveBeenCalledTimes(1)
    const [, options] = toastMocks.error.mock.calls[0]
    expect(options).toMatchObject({
      id: DAEMON_STATUS_TOAST_ID,
      duration: Infinity,
      dismissible: true
    })
    expect(options.action.label).toBe('Retry')
  })

  it('wires the toast action to the relaunch IPC and surfaces a failed retry', async () => {
    relaunchMock.mockResolvedValue({ success: false, error: 'still broken' })
    await mountBridge()
    await emit(makeStatus({ state: 'failed', cause: 'launch-failed', updatedAt: 2 }))

    const [, options] = toastMocks.error.mock.calls[0]
    toastMocks.error.mockClear()
    await act(async () => {
      options.action.onClick()
    })

    expect(relaunchMock).toHaveBeenCalledTimes(1)
    expect(toastMocks.error).toHaveBeenCalledWith(
      'Daemon restart failed.',
      expect.objectContaining({ description: 'still broken' })
    )
  })

  it('dismisses the sticky toast and celebrates when the daemon recovers', async () => {
    await mountBridge()
    await emit(makeStatus({ state: 'degraded-fallback', cause: 'spawn-unhealthy', updatedAt: 2 }))
    await emit(makeStatus({ state: 'running', updatedAt: 3 }))

    expect(toastMocks.dismiss).toHaveBeenCalledWith(DAEMON_STATUS_TOAST_ID)
    expect(toastMocks.success).toHaveBeenCalledTimes(1)
  })

  it('dismisses without celebrating while a retry is in flight, then re-shows on repeat failure', async () => {
    await mountBridge()
    await emit(makeStatus({ state: 'failed', cause: 'launch-failed', updatedAt: 2 }))
    expect(toastMocks.error).toHaveBeenCalledTimes(1)

    await emit(makeStatus({ state: 'starting', updatedAt: 3 }))
    expect(toastMocks.dismiss).toHaveBeenCalledWith(DAEMON_STATUS_TOAST_ID)
    expect(toastMocks.success).not.toHaveBeenCalled()

    await emit(makeStatus({ state: 'failed', cause: 'launch-failed', updatedAt: 4 }))
    expect(toastMocks.error).toHaveBeenCalledTimes(2)
  })

  it('stays silent through a healthy startup', async () => {
    await mountBridge()
    await emit(makeStatus({ state: 'running', updatedAt: 2 }))

    expect(toastMocks.error).not.toHaveBeenCalled()
    expect(toastMocks.success).not.toHaveBeenCalled()
    expect(toastMocks.dismiss).not.toHaveBeenCalled()
  })
})
