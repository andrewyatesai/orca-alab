import { beforeEach, describe, expect, it, vi } from 'vitest'

const {
  handleMock,
  removeHandlerMock,
  getDaemonProviderMock,
  initDaemonPtyProviderMock,
  restartDaemonMock,
  getLocalPtyProviderMock,
  localProvider
} = vi.hoisted(() => {
  const localProvider = {
    listProcesses: vi.fn(async (): Promise<{ id: string }[]> => []),
    shutdown: vi.fn(async (_id: string, _opts: { immediate?: boolean }) => {})
  }
  return {
    handleMock: vi.fn(),
    removeHandlerMock: vi.fn(),
    getDaemonProviderMock: vi.fn((): unknown => null),
    initDaemonPtyProviderMock: vi.fn(async () => {}),
    restartDaemonMock: vi.fn(async () => ({ killedCount: 0 })),
    getLocalPtyProviderMock: vi.fn(() => localProvider),
    localProvider
  }
})

vi.mock('electron', () => ({
  ipcMain: { handle: handleMock, removeHandler: removeHandlerMock },
  BrowserWindow: { getAllWindows: () => [] }
}))

vi.mock('../daemon/daemon-init', () => ({
  getDaemonProvider: getDaemonProviderMock,
  initDaemonPtyProvider: initDaemonPtyProviderMock,
  restartDaemon: restartDaemonMock
}))

// Why: daemon-status lazily imports ./pty only on the relaunch path; mock it so
// the test never loads the real (native-dep-heavy) PTY IPC module.
vi.mock('./pty', () => ({
  getLocalPtyProvider: getLocalPtyProviderMock
}))

import { registerDaemonStatusHandlers, relaunchDaemonForRecovery } from './daemon-status'
import {
  getDaemonRuntimeStatus,
  resetDaemonRuntimeStatusForTest,
  setDaemonRuntimeStatus
} from './daemon-status-registry'

type Handler = (event: unknown, args?: unknown) => unknown

function buildHandlerMap(): Record<string, Handler> {
  const map: Record<string, Handler> = {}
  for (const [channel, handler] of handleMock.mock.calls as [string, Handler][]) {
    map[channel] = handler
  }
  return map
}

describe('daemon-status IPC handlers', () => {
  beforeEach(() => {
    resetDaemonRuntimeStatusForTest()
    handleMock.mockClear()
    removeHandlerMock.mockClear()
    getDaemonProviderMock.mockReset()
    getDaemonProviderMock.mockReturnValue(null)
    initDaemonPtyProviderMock.mockReset()
    initDaemonPtyProviderMock.mockResolvedValue(undefined)
    restartDaemonMock.mockReset()
    restartDaemonMock.mockResolvedValue({ killedCount: 0 })
    getLocalPtyProviderMock.mockClear()
    localProvider.listProcesses.mockReset()
    localProvider.listProcesses.mockResolvedValue([])
    localProvider.shutdown.mockReset()
    localProvider.shutdown.mockResolvedValue(undefined)
  })

  it('re-registers both channels idempotently', () => {
    registerDaemonStatusHandlers()
    registerDaemonStatusHandlers()

    expect(removeHandlerMock).toHaveBeenCalledWith('daemon:status:get')
    expect(removeHandlerMock).toHaveBeenCalledWith('daemon:status:relaunch')
    const channels = handleMock.mock.calls.map(([channel]) => channel)
    expect(channels.filter((c) => c === 'daemon:status:get')).toHaveLength(2)
    expect(channels.filter((c) => c === 'daemon:status:relaunch')).toHaveLength(2)
  })

  it('daemon:status:get returns the registry snapshot', async () => {
    registerDaemonStatusHandlers()
    setDaemonRuntimeStatus('failed', { cause: 'launch-failed', detail: 'no binary' })

    const result = await buildHandlerMap()['daemon:status:get']({})
    expect(result).toMatchObject({ state: 'failed', cause: 'launch-failed', detail: 'no binary' })
  })

  it('relaunch is a no-op success while the daemon is running', async () => {
    setDaemonRuntimeStatus('running')

    await expect(relaunchDaemonForRecovery()).resolves.toEqual({ success: true })
    expect(restartDaemonMock).not.toHaveBeenCalled()
    expect(initDaemonPtyProviderMock).not.toHaveBeenCalled()
  })

  it('relaunch is a no-op while an init attempt is already in flight (starting, no provider)', async () => {
    // Registry resets to 'starting' — the state during startup init. The
    // settings Restart button now routes here, so racing a second init
    // (double-spawning daemons) must be impossible.
    await expect(relaunchDaemonForRecovery()).resolves.toEqual({ success: true })
    expect(restartDaemonMock).not.toHaveBeenCalled()
    expect(initDaemonPtyProviderMock).not.toHaveBeenCalled()
  })

  it('relaunch reuses restartDaemon when a (degraded) provider is installed', async () => {
    setDaemonRuntimeStatus('degraded-fallback', { cause: 'spawn-unhealthy' })
    getDaemonProviderMock.mockReturnValue({})

    await expect(relaunchDaemonForRecovery()).resolves.toEqual({ success: true })
    expect(restartDaemonMock).toHaveBeenCalledTimes(1)
    expect(initDaemonPtyProviderMock).not.toHaveBeenCalled()
  })

  it('relaunch after total failure kills local fallback sessions before re-running init', async () => {
    setDaemonRuntimeStatus('failed', { cause: 'launch-failed', detail: 'boom' })
    localProvider.listProcesses.mockResolvedValue([{ id: 'pty-1' }, { id: 'pty-2' }])
    const order: string[] = []
    localProvider.shutdown.mockImplementation(async (id: string) => {
      order.push(`shutdown:${id}`)
    })
    initDaemonPtyProviderMock.mockImplementation(async () => {
      order.push('init')
    })

    await expect(relaunchDaemonForRecovery()).resolves.toEqual({ success: true })

    expect(localProvider.shutdown).toHaveBeenCalledWith('pty-1', { immediate: true })
    expect(localProvider.shutdown).toHaveBeenCalledWith('pty-2', { immediate: true })
    expect(order.at(-1)).toBe('init')
    expect(restartDaemonMock).not.toHaveBeenCalled()
  })

  it('relaunch still re-runs init when a fallback session refuses to shut down', async () => {
    setDaemonRuntimeStatus('failed', { cause: 'launch-failed' })
    localProvider.listProcesses.mockResolvedValue([{ id: 'stuck' }])
    localProvider.shutdown.mockRejectedValue(new Error('EPERM'))

    await expect(relaunchDaemonForRecovery()).resolves.toEqual({ success: true })
    expect(initDaemonPtyProviderMock).toHaveBeenCalledTimes(1)
  })

  it('relaunch reports failure without throwing when init throws again', async () => {
    setDaemonRuntimeStatus('failed', { cause: 'launch-failed' })
    initDaemonPtyProviderMock.mockRejectedValue(new Error('still broken'))

    await expect(relaunchDaemonForRecovery()).resolves.toEqual({
      success: false,
      error: 'still broken'
    })
  })

  it('coalesces concurrent relaunch requests onto one attempt', async () => {
    setDaemonRuntimeStatus('failed', { cause: 'launch-failed' })
    let release!: () => void
    initDaemonPtyProviderMock.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve
        })
    )

    const first = relaunchDaemonForRecovery()
    const second = relaunchDaemonForRecovery()
    // The relaunch path awaits async steps (lazy ./pty import, listProcesses)
    // before init runs — wait for the attempt to start before releasing it.
    await vi.waitFor(() => expect(initDaemonPtyProviderMock).toHaveBeenCalled())
    release()
    await expect(first).resolves.toEqual({ success: true })
    await expect(second).resolves.toEqual({ success: true })
    expect(initDaemonPtyProviderMock).toHaveBeenCalledTimes(1)
  })

  it('does not report success stale-y: registry stays authoritative after relaunch failure', async () => {
    setDaemonRuntimeStatus('failed', { cause: 'launch-failed', detail: 'boom' })
    initDaemonPtyProviderMock.mockRejectedValue(new Error('boom'))

    await relaunchDaemonForRecovery()
    // The registry write on failure belongs to initDaemonPtyProvider (mocked
    // here) — the handler must not overwrite it with a success state.
    expect(getDaemonRuntimeStatus().state).toBe('failed')
  })
})
