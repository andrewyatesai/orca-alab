import { beforeEach, describe, expect, it, vi } from 'vitest'

const { getAllWindowsMock, makeFakeWindow } = vi.hoisted(() => {
  const getAllWindowsMock = vi.fn((): unknown[] => [])
  const makeFakeWindow = (destroyed = false) => {
    const send = vi.fn()
    return {
      isDestroyed: () => destroyed,
      webContents: { send },
      send
    }
  }
  return { getAllWindowsMock, makeFakeWindow }
})

vi.mock('electron', () => ({
  BrowserWindow: { getAllWindows: getAllWindowsMock }
}))

import {
  DAEMON_STATUS_CHANGED_CHANNEL,
  getDaemonRuntimeStatus,
  resetDaemonRuntimeStatusForTest,
  setDaemonRuntimeStatus
} from './daemon-status-registry'

describe('daemon-status-registry', () => {
  beforeEach(() => {
    resetDaemonRuntimeStatusForTest()
    getAllWindowsMock.mockReset()
    getAllWindowsMock.mockReturnValue([])
  })

  it('starts in the starting state with no cause or detail', () => {
    const status = getDaemonRuntimeStatus()
    expect(status.state).toBe('starting')
    expect(status.cause).toBeNull()
    expect(status.detail).toBeNull()
    expect(status.updatedAt).toBeGreaterThan(0)
  })

  it('stores state, cause, and detail on set', () => {
    setDaemonRuntimeStatus('failed', { cause: 'launch-failed', detail: 'binary not found' })
    expect(getDaemonRuntimeStatus()).toMatchObject({
      state: 'failed',
      cause: 'launch-failed',
      detail: 'binary not found'
    })
  })

  it('broadcasts the new status to every live window', () => {
    const live = makeFakeWindow()
    const destroyed = makeFakeWindow(true)
    getAllWindowsMock.mockReturnValue([live, destroyed])

    setDaemonRuntimeStatus('degraded-fallback', { cause: 'spawn-unhealthy' })

    expect(live.send).toHaveBeenCalledTimes(1)
    const [channel, payload] = live.send.mock.calls[0]
    expect(channel).toBe(DAEMON_STATUS_CHANGED_CHANNEL)
    expect(payload).toMatchObject({ state: 'degraded-fallback', cause: 'spawn-unhealthy' })
    expect(destroyed.send).not.toHaveBeenCalled()
  })

  it('deduplicates identical status writes', () => {
    const win = makeFakeWindow()
    getAllWindowsMock.mockReturnValue([win])

    setDaemonRuntimeStatus('running')
    setDaemonRuntimeStatus('running')

    expect(win.send).toHaveBeenCalledTimes(1)
  })

  it('re-broadcasts when only the cause or detail changes', () => {
    const win = makeFakeWindow()
    getAllWindowsMock.mockReturnValue([win])

    setDaemonRuntimeStatus('failed', { cause: 'launch-failed', detail: 'a' })
    setDaemonRuntimeStatus('failed', { cause: 'launch-failed', detail: 'b' })

    expect(win.send).toHaveBeenCalledTimes(2)
  })
})
