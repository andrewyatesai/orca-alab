import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Focused coverage for the F8 status-reporting hooks: every init/restart
// outcome must land in the daemon-status registry. The full 7-step restart
// sequencing is covered by daemon-init.test.ts; here the boundary mocks are
// trimmed to just what the status wrapper needs.

const {
  getPathMock,
  setDaemonRuntimeStatusMock,
  ensureRunningOverrides,
  spawnerHandleModes,
  spawnerInstances,
  localFallbackProvider,
  setLocalPtyProviderMock,
  unbindLocalProviderListenersMock,
  rebindLocalProviderListenersMock,
  checkDaemonHealthMock,
  killStaleDaemonMock,
  daemonClientMock
} = vi.hoisted(() => {
  const makeUnsubscribe = () => () => {}
  return {
    getPathMock: vi.fn(() => '/fake/userData'),
    setDaemonRuntimeStatusMock: vi.fn(),
    // Each entry is consumed by one DaemonSpawner.ensureRunning call: a thrown
    // value fails the launch; otherwise the socket info is returned.
    ensureRunningOverrides: [] as (() => Promise<{ socketPath: string; tokenPath: string }>)[],
    // Consumed per-spawner: the handle mode reported after ensureRunning.
    spawnerHandleModes: [] as (undefined | 'degraded-new-pty-fallback')[],
    spawnerInstances: [] as unknown[],
    localFallbackProvider: {
      onData: vi.fn(makeUnsubscribe),
      onExit: vi.fn(makeUnsubscribe),
      onReplay: vi.fn(makeUnsubscribe),
      listProcesses: vi.fn(async () => []),
      shutdown: vi.fn(async () => {})
    },
    setLocalPtyProviderMock: vi.fn(),
    unbindLocalProviderListenersMock: vi.fn(),
    rebindLocalProviderListenersMock: vi.fn(),
    checkDaemonHealthMock: vi.fn(async () => 'healthy' as const),
    killStaleDaemonMock: vi.fn(async () => false),
    daemonClientMock: vi.fn(() => ({
      ensureConnected: vi.fn(async () => {}),
      request: vi.fn(async () => ({ sessions: [] })),
      disconnect: vi.fn()
    }))
  }
})

vi.mock('electron', () => ({
  app: {
    isPackaged: false,
    getPath: getPathMock,
    getAppPath: () => '/fake/app',
    getVersion: () => '1.2.3'
  }
}))

vi.mock('fs', () => ({
  mkdirSync: vi.fn(),
  existsSync: () => false,
  unlinkSync: vi.fn(),
  writeFileSync: vi.fn()
}))

vi.mock('child_process', () => ({ fork: vi.fn(), spawn: vi.fn() }))

vi.mock('net', () => ({
  connect: vi.fn(() => {
    // probeSocket stub: fire 'error' so every socket probe resolves false.
    const self = {
      on(event: string, cb: () => void) {
        if (event === 'error') {
          queueMicrotask(cb)
        }
        return self
      },
      removeListener() {
        return self
      },
      destroy() {}
    }
    return self
  })
}))

vi.mock('./daemon-health', () => ({
  checkDaemonHealth: checkDaemonHealthMock,
  getDaemonLaunchIdentity: vi.fn(async () => 'match'),
  getMacDaemonSystemResolverHealth: vi.fn(async () => 'healthy'),
  isDaemonStaleForCurrentBundle: vi.fn(async () => false),
  killStaleDaemon: killStaleDaemonMock,
  getProcessStartedAtMs: vi.fn(() => 123)
}))

vi.mock('./client', () => ({ DaemonClient: daemonClientMock }))

vi.mock('./daemon-spawner', () => ({
  DaemonSpawner: class MockDaemonSpawner {
    readonly ensureRunning: ReturnType<typeof vi.fn>
    readonly resetHandle = vi.fn()
    readonly shutdown = vi.fn(async () => {})
    readonly getHandle: ReturnType<typeof vi.fn>
    constructor() {
      const mode = spawnerHandleModes.shift()
      this.getHandle = vi.fn(() => (mode ? { mode, shutdown: async () => {} } : null))
      this.ensureRunning = vi.fn(async () => {
        const override = ensureRunningOverrides.shift()
        if (override) {
          return override()
        }
        return { socketPath: '/fake/socket', tokenPath: '/fake/token' }
      })
      spawnerInstances.push(this)
    }
  },
  getDaemonSocketPath: (_dir: string, version?: number) => `/fake/daemon-v${version ?? 0}.sock`,
  getDaemonTokenPath: (_dir: string, version?: number) => `/fake/daemon-v${version ?? 0}.token`,
  getDaemonPidPath: (_dir: string, version?: number) => `/fake/daemon-v${version ?? 0}.pid`,
  serializeDaemonPidFile: (obj: unknown) => JSON.stringify(obj)
}))

vi.mock('./daemon-pty-adapter', () => ({
  DaemonPtyAdapter: class MockDaemonPtyAdapter {
    readonly protocolVersion = 0
    readonly getActiveSessionIds = vi.fn(() => [] as string[])
    readonly fanoutSyntheticExits = vi.fn()
    readonly listProcesses = vi.fn(async () => [])
    readonly listSessions = vi.fn(async () => [])
    readonly shutdown = vi.fn(async () => {})
    readonly dispose = vi.fn()
    readonly disconnectOnly = vi.fn(async () => {})
    // Why: upstream #9277 makes daemon-init establish a lifecycle lease on the permanent adapter pair.
    readonly establishLifecycleLease = vi.fn(async () => {})
    readonly onData = vi.fn(() => () => {})
    readonly onExit = vi.fn(() => () => {})
    readonly onReplay = vi.fn(() => () => {})
  }
}))

vi.mock('../ipc/pty', () => ({
  getLocalPtyProvider: vi.fn(() => localFallbackProvider),
  setLocalPtyProvider: setLocalPtyProviderMock,
  unbindLocalProviderListeners: unbindLocalProviderListenersMock,
  rebindLocalProviderListeners: rebindLocalProviderListenersMock
}))

vi.mock('../ipc/daemon-status-registry', () => ({
  setDaemonRuntimeStatus: setDaemonRuntimeStatusMock
}))

vi.mock('./history-store-layout', () => ({
  prepareDaemonSessionStoreRoot: (root: string) => root
}))

vi.mock('./history-retention', () => ({
  scheduleDaemonSessionHistoryGc: vi.fn()
}))

// Return type inferred: naming it would need an `import()` type annotation,
// which the consistent-type-imports lint forbids.
async function importFresh() {
  vi.resetModules()
  ensureRunningOverrides.length = 0
  spawnerHandleModes.length = 0
  spawnerInstances.length = 0
  return import('./daemon-init')
}

function reportedStatuses(): [string, { cause?: string; detail?: string } | undefined][] {
  return setDaemonRuntimeStatusMock.mock.calls as [
    string,
    { cause?: string; detail?: string } | undefined
  ][]
}

describe('daemon-init status reporting (F8)', () => {
  beforeEach(() => {
    setDaemonRuntimeStatusMock.mockClear()
    checkDaemonHealthMock.mockClear()
    checkDaemonHealthMock.mockResolvedValue('healthy')
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('reports starting then running on a successful init', async () => {
    const mod = await importFresh()
    await mod.initDaemonPtyProvider()

    const calls = reportedStatuses()
    expect(calls[0][0]).toBe('starting')
    expect(calls.at(-1)).toEqual(['running'])
  })

  it('reports failed with launch-failed and the error detail when init throws', async () => {
    const mod = await importFresh()
    ensureRunningOverrides.push(async () => {
      throw new Error('orca-daemon binary not found')
    })

    await expect(mod.initDaemonPtyProvider()).rejects.toThrow('orca-daemon binary not found')
    expect(setDaemonRuntimeStatusMock).toHaveBeenLastCalledWith('failed', {
      cause: 'launch-failed',
      detail: 'orca-daemon binary not found'
    })
  })

  it('reports degraded-fallback (spawn-unhealthy) when the preserved daemon cannot spawn PTYs', async () => {
    const mod = await importFresh()
    spawnerHandleModes.push('degraded-new-pty-fallback')

    await mod.initDaemonPtyProvider()

    expect(setDaemonRuntimeStatusMock).toHaveBeenLastCalledWith('degraded-fallback', {
      cause: 'spawn-unhealthy'
    })
  })

  it('reports degraded-fallback (startup-timeout) when the fail-open abort wins the race', async () => {
    const mod = await importFresh()
    const controller = new AbortController()
    controller.abort()

    await mod.initDaemonPtyProvider(controller.signal)

    expect(setDaemonRuntimeStatusMock).toHaveBeenLastCalledWith('degraded-fallback', {
      cause: 'startup-timeout'
    })
    // The aborted attempt must not have installed a provider.
    expect(mod.getDaemonProvider()).toBeNull()
  })

  it('reports running after a successful restart', async () => {
    const mod = await importFresh()
    await mod.initDaemonPtyProvider()
    setDaemonRuntimeStatusMock.mockClear()

    await mod.restartDaemon()

    expect(setDaemonRuntimeStatusMock).toHaveBeenLastCalledWith('running')
  })

  it('reports failed with restart-failed when the restart respawn throws', async () => {
    const mod = await importFresh()
    await mod.initDaemonPtyProvider()
    setDaemonRuntimeStatusMock.mockClear()
    ensureRunningOverrides.push(async () => {
      throw new Error('respawn exploded')
    })

    await expect(mod.restartDaemon()).rejects.toThrow('respawn exploded')
    expect(setDaemonRuntimeStatusMock).toHaveBeenLastCalledWith('failed', {
      cause: 'restart-failed',
      detail: 'respawn exploded'
    })
  })

  // Why: after a total launch failure the registry carries the actionable
  // launch error (e.g. "binary not found"). A restart call with no provider
  // must reject on its precondition WITHOUT writing 'restart-failed' over it —
  // the settings banner and status-bar tooltip surface that detail string.
  it('does not clobber the launch-failed detail when restart is invoked with no provider', async () => {
    const mod = await importFresh()
    ensureRunningOverrides.push(async () => {
      throw new Error('orca-daemon binary not found')
    })
    await expect(mod.initDaemonPtyProvider()).rejects.toThrow('orca-daemon binary not found')
    setDaemonRuntimeStatusMock.mockClear()

    await expect(mod.restartDaemon()).rejects.toThrow(
      'restartDaemon called before initDaemonPtyProvider'
    )
    expect(setDaemonRuntimeStatusMock).not.toHaveBeenCalled()
  })

  it('reports running after a restart that replaces a degraded provider', async () => {
    const mod = await importFresh()
    spawnerHandleModes.push('degraded-new-pty-fallback')
    await mod.initDaemonPtyProvider()
    setDaemonRuntimeStatusMock.mockClear()

    await mod.restartDaemon()

    // The restart installed a healthy (non-degraded) provider, so the
    // provider-keyed success hook must report 'running', not degraded.
    expect(setDaemonRuntimeStatusMock).toHaveBeenLastCalledWith('running')
  })
})
