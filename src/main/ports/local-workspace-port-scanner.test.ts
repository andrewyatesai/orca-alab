import { afterEach, describe, expect, it, vi } from 'vitest'
import path from 'node:path'
import {
  attributePortToWorkspace,
  isContainerProcess,
  parseLsofListeningOutput,
  parseNetstatListeningOutput,
  parseProcNetTcp,
  parseSsListeningOutput,
  resetWorkspacePortScanTimeoutBackoffForTests,
  scanWorkspacePorts
} from './local-workspace-port-scanner'

const execFileMock = vi.hoisted(() => vi.fn())

vi.mock('child_process', () => ({
  execFile: execFileMock
}))

const fsPromisesMock = vi.hoisted(() => ({
  readFile: vi.fn(),
  readdir: vi.fn(),
  readlink: vi.fn()
}))

vi.mock('node:fs/promises', () => fsPromisesMock)

const worktrees = [
  {
    id: 'repo::/repo',
    repoId: 'repo',
    displayName: 'main',
    path: '/repo'
  },
  {
    id: 'repo::/repo/worktrees/feature',
    repoId: 'repo',
    displayName: 'feature',
    path: '/repo/worktrees/feature'
  }
]

describe('local workspace port scanner parsing', () => {
  it('parses lsof field output into listening ports', () => {
    const ports = parseLsofListeningOutput(
      ['p123', 'cnode', 'n127.0.0.1:5173', 'p456', 'cnginx', 'n*:8080'].join('\n')
    )

    expect(ports).toEqual([
      { pid: 123, processName: 'node', host: '127.0.0.1', port: 5173 },
      { pid: 456, processName: 'nginx', host: '*', port: 8080 }
    ])
  })

  it('parses multiple lsof listening ports for the same process', () => {
    const ports = parseLsofListeningOutput(
      ['p123', 'cnode', 'n127.0.0.1:5173', 'n127.0.0.1:55173'].join('\n')
    )

    expect(ports).toEqual([
      { pid: 123, processName: 'node', host: '127.0.0.1', port: 5173 },
      { pid: 123, processName: 'node', host: '127.0.0.1', port: 55173 }
    ])
  })

  it('parses Windows netstat listening rows', () => {
    const ports = parseNetstatListeningOutput(
      [
        'Proto  Local Address          Foreign Address        State           PID',
        'TCP    127.0.0.1:3000         0.0.0.0:0              LISTENING       4242',
        'TCP    [::]:5173              [::]:0                 LISTENING       5151'
      ].join('\n')
    )

    expect(ports).toEqual([
      { host: '127.0.0.1', port: 3000, pid: 4242 },
      { host: '::', port: 5173, pid: 5151 }
    ])
  })

  it('parses Windows netstat rows without whitespace regex splitting', () => {
    const splitSpy = vi.spyOn(String.prototype, 'split')
    const ports = parseNetstatListeningOutput(
      'TCP    127.0.0.1:3000         0.0.0.0:0              LISTENING       4242'
    )
    const usedWhitespaceFieldSplit = splitSpy.mock.calls.some(
      ([separator]) => separator instanceof RegExp && separator.source.includes('\\s+')
    )
    splitSpy.mockRestore()

    expect(ports).toEqual([{ host: '127.0.0.1', port: 3000, pid: 4242 }])
    expect(usedWhitespaceFieldSplit).toBe(false)
  })

  it('parses Linux proc tcp listeners', () => {
    const ports = parseProcNetTcp(
      [
        '  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode',
        '   0: 0100007F:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 12345 1 0000000000000000 100 0 0 10 0'
      ].join('\n')
    )

    expect(ports).toEqual([{ host: '127.0.0.1', port: 3000, inode: 12345 }])
  })

  it('parses Linux proc rows without whitespace regex splitting', () => {
    const splitSpy = vi.spyOn(String.prototype, 'split')
    const ports = parseProcNetTcp(
      [
        '  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode',
        '   0: 0100007F:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 12345'
      ].join('\n')
    )
    const usedWhitespaceFieldSplit = splitSpy.mock.calls.some(
      ([separator]) => separator instanceof RegExp && separator.source.includes('\\s+')
    )
    splitSpy.mockRestore()

    expect(ports).toEqual([{ host: '127.0.0.1', port: 3000, inode: 12345 }])
    expect(usedWhitespaceFieldSplit).toBe(false)
  })

  it('parses ss -lntH listening rows across address formats', () => {
    const ports = parseSsListeningOutput(
      [
        'LISTEN 0      511          127.0.0.1:5173       0.0.0.0:*',
        'LISTEN 0      4096            [::]:8080             [::]:*',
        'LISTEN 0      128                  *:3000               *:*',
        'LISTEN 0      128                :::22                 :::*',
        'ESTAB  0      0            127.0.0.1:44444     127.0.0.1:5173'
      ].join('\n')
    )

    expect(ports).toEqual([
      { host: '127.0.0.1', port: 5173 },
      { host: '::', port: 8080 },
      { host: '*', port: 3000 },
      { host: '::', port: 22 }
    ])
  })

  it('skips the header row when ss ignores -H', () => {
    const ports = parseSsListeningOutput(
      [
        'State  Recv-Q Send-Q Local Address:Port  Peer Address:Port Process',
        'LISTEN 0      511          0.0.0.0:3000        0.0.0.0:*'
      ].join('\n')
    )

    expect(ports).toEqual([{ host: '0.0.0.0', port: 3000 }])
  })

  it('captures pid and process name when ss includes a process column', () => {
    const ports = parseSsListeningOutput(
      'LISTEN 0 511 127.0.0.1:3000 0.0.0.0:* users:(("node",pid=4242,fd=23))'
    )

    expect(ports).toEqual([{ host: '127.0.0.1', port: 3000, pid: 4242, processName: 'node' }])
  })
})

describe('attributePortToWorkspace', () => {
  it('uses cwd ancestry and picks the deepest matching worktree', () => {
    const owner = attributePortToWorkspace(
      { cwd: '/repo/worktrees/feature/packages/app', commandLine: 'node server.js' },
      worktrees
    )

    expect(owner).toMatchObject({
      worktreeId: 'repo::/repo/worktrees/feature',
      displayName: 'feature',
      confidence: 'cwd'
    })
  })

  it('falls back to command-line path evidence', () => {
    const commandPath = path.posix.resolve('/repo/worktrees/feature/node_modules/vite/bin/vite.js')
    const owner = attributePortToWorkspace({ commandLine: `node ${commandPath}` }, worktrees)

    expect(owner).toMatchObject({
      worktreeId: 'repo::/repo/worktrees/feature',
      confidence: 'command'
    })
  })

  it('requires command-line path boundary evidence', () => {
    const owner = attributePortToWorkspace(
      { commandLine: `node ${path.posix.resolve('/repo/worktrees/feature-other/server.js')}` },
      [worktrees[1]]
    )

    expect(owner).toBeUndefined()
  })

  it('keeps path case significant on case-sensitive platforms', () => {
    const owner = attributePortToWorkspace({ cwd: '/Repo/worktrees/feature' }, worktrees)

    if (process.platform === 'win32') {
      expect(owner).toMatchObject({ worktreeId: 'repo::/repo/worktrees/feature' })
    } else {
      expect(owner).toBeUndefined()
    }
  })

  it('does not guess when there is no worktree evidence', () => {
    const owner = attributePortToWorkspace({ cwd: '/Applications/ContainerRuntime.app' }, worktrees)

    expect(owner).toBeUndefined()
  })
})

describe('container process classification', () => {
  it('detects common container listener owners without workspace attribution', () => {
    expect(isContainerProcess({ processName: 'com.container.backend' })).toBe(true)
    expect(isContainerProcess({ processName: 'com.vendor.backend' })).toBe(true)
    expect(isContainerProcess({ commandLine: '/usr/bin/container-runtime port-forward' })).toBe(
      true
    )
    expect(isContainerProcess({ processName: 'node', commandLine: 'node server.js' })).toBe(false)
  })
})

describe('scanWorkspacePorts attribution work', () => {
  afterEach(() => {
    resetWorkspacePortScanTimeoutBackoffForTests()
    vi.restoreAllMocks()
    execFileMock.mockReset()
  })

  it('normalizes worktree paths once per scan instead of once per port phase', async () => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue('darwin')
    const win32ResolveSpy = vi.spyOn(path.win32, 'resolve')
    const posixResolveSpy = vi.spyOn(path.posix, 'resolve')
    const invokeCallback = (callback: unknown, stdout: string): void => {
      if (typeof callback !== 'function') {
        throw new Error('missing execFile callback')
      }
      const execCallback = callback as (error: Error | null, stdout: string) => void
      execCallback(null, stdout)
    }
    execFileMock.mockImplementation(
      (command: string, args: string[], _options: unknown, callback: unknown) => {
        if (command === 'lsof' && args.includes('-iTCP')) {
          invokeCallback(
            callback,
            ['p123', 'cnode', 'n127.0.0.1:3000', 'p124', 'cnode', 'n127.0.0.1:3001'].join('\n')
          )
        } else if (command === 'lsof') {
          invokeCallback(
            callback,
            ['p123', 'n/repo/service', 'p124', 'n/repo/worktrees/feature/app'].join('\n')
          )
        } else if (command === 'ps') {
          invokeCallback(
            callback,
            [
              '123 node /repo/service/server.js',
              '124 node /repo/worktrees/feature/app/server.js'
            ].join('\n')
          )
        } else {
          invokeCallback(callback, '')
        }
        return { kill: vi.fn() }
      }
    )

    const scan = await scanWorkspacePorts(worktrees, {
      lookup: () => undefined,
      reconcileScan: vi.fn()
    })

    expect(scan.ports.filter((port) => port.kind === 'workspace')).toHaveLength(2)
    const win32WorktreePathResolveCalls = win32ResolveSpy.mock.calls.filter(
      ([input]) => input === '/repo' || input === '/repo/worktrees/feature'
    )
    const posixWorktreePathResolveCalls = posixResolveSpy.mock.calls.filter(
      ([input]) => input === '/repo' || input === '/repo/worktrees/feature'
    )
    expect(win32WorktreePathResolveCalls).toHaveLength(0)
    expect(posixWorktreePathResolveCalls).toHaveLength(worktrees.length)
  })
})

describe('scanWorkspacePorts command timeout', () => {
  afterEach(() => {
    vi.useRealTimers()
    resetWorkspacePortScanTimeoutBackoffForTests()
    vi.restoreAllMocks()
    execFileMock.mockReset()
  })

  it('returns an unavailable scan when lsof never reports completion', async () => {
    vi.useFakeTimers()
    vi.spyOn(process, 'platform', 'get').mockReturnValue('darwin')
    const killMock = vi.fn()
    execFileMock.mockImplementation(() => ({ kill: killMock }))

    let settled = false
    const scanPromise = scanWorkspacePorts([], {
      lookup: () => undefined,
      reconcileScan: vi.fn()
    }).then((scan) => {
      settled = true
      return scan
    })

    await vi.advanceTimersByTimeAsync(4_000)

    expect(settled).toBe(true)
    await expect(scanPromise).resolves.toMatchObject({
      platform: 'darwin',
      ports: [],
      unavailableReason: 'Port scanning is unavailable on darwin.'
    })
    expect(killMock).toHaveBeenCalled()
  })

  it('backs off after a command timeout instead of launching lsof on every scan tick', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(1_000)
    vi.spyOn(process, 'platform', 'get').mockReturnValue('darwin')
    const killMock = vi.fn()
    execFileMock.mockImplementation(() => ({ kill: killMock }))

    const firstScanPromise = scanWorkspacePorts([], {
      lookup: () => undefined,
      reconcileScan: vi.fn()
    })

    await vi.advanceTimersByTimeAsync(4_000)
    await expect(firstScanPromise).resolves.toMatchObject({
      platform: 'darwin',
      ports: [],
      unavailableReason: 'Port scanning is unavailable on darwin.'
    })
    expect(execFileMock).toHaveBeenCalledTimes(1)

    const cooldownScans = await Promise.all(
      Array.from({ length: 10 }, () =>
        scanWorkspacePorts([], {
          lookup: () => undefined,
          reconcileScan: vi.fn()
        })
      )
    )

    expect(cooldownScans).toHaveLength(10)
    expect(cooldownScans[0]).toMatchObject({
      platform: 'darwin',
      ports: []
    })
    expect(
      cooldownScans.every((scan) => scan.unavailableReason?.includes('temporarily paused'))
    ).toBe(true)
    expect(execFileMock).toHaveBeenCalledTimes(1)

    vi.setSystemTime(65_001)
    await vi.advanceTimersByTimeAsync(0)
    execFileMock.mockImplementation(
      (_command: string, args: string[], _options: unknown, callback: unknown) => {
        const execCallback = callback as (error: Error | null, stdout: string) => void
        const output = args.includes('-iTCP') ? 'p123\ncnode\nn127.0.0.1:3000' : ''
        execCallback(null, output)
        return { kill: vi.fn() }
      }
    )

    const recoveredScan = await scanWorkspacePorts([], {
      lookup: () => undefined,
      reconcileScan: vi.fn()
    })

    expect(recoveredScan.unavailableReason).toBeUndefined()
    expect(execFileMock).toHaveBeenCalledTimes(4)
  })
})

describe('scanWorkspacePorts Linux /proc fallback chain', () => {
  const invokeCallback = (callback: unknown, error: Error | null, stdout: string): void => {
    if (typeof callback !== 'function') {
      throw new Error('missing execFile callback')
    }
    ;(callback as (error: Error | null, stdout: string) => void)(error, stdout)
  }
  const rejectAllProcReads = (): void => {
    fsPromisesMock.readFile.mockRejectedValue(new Error('EACCES'))
    fsPromisesMock.readdir.mockRejectedValue(new Error('EACCES'))
    fsPromisesMock.readlink.mockRejectedValue(new Error('EACCES'))
  }
  const procNetTcpFixture = [
    '  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode',
    '   0: 0100007F:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 12345 1 0000000000000000 100 0 0 10 0'
  ].join('\n')

  afterEach(() => {
    resetWorkspacePortScanTimeoutBackoffForTests()
    vi.restoreAllMocks()
    execFileMock.mockReset()
    fsPromisesMock.readFile.mockReset()
    fsPromisesMock.readdir.mockReset()
    fsPromisesMock.readlink.mockReset()
  })

  it('falls back to ss when /proc/net/tcp is unavailable', async () => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue('linux')
    rejectAllProcReads()
    execFileMock.mockImplementation(
      (command: string, _args: string[], _options: unknown, callback: unknown) => {
        invokeCallback(
          callback,
          null,
          command === 'ss'
            ? [
                'LISTEN 0      511          127.0.0.1:5173       0.0.0.0:*',
                'LISTEN 0      4096            [::]:8080             [::]:*'
              ].join('\n')
            : ''
        )
        return { kill: vi.fn() }
      }
    )

    const scan = await scanWorkspacePorts([], { lookup: () => undefined, reconcileScan: vi.fn() })

    expect(scan.unavailableReason).toBeUndefined()
    expect(scan.ports.map((port) => port.port)).toEqual([5173, 8080])
    expect(execFileMock.mock.calls.map(([command]) => command)).toEqual(['ss'])
  })

  it('falls back to lsof when /proc and ss are both unavailable', async () => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue('linux')
    rejectAllProcReads()
    execFileMock.mockImplementation(
      (command: string, _args: string[], _options: unknown, callback: unknown) => {
        if (command === 'ss') {
          invokeCallback(callback, new Error('ss: invalid option -- "H"'), '')
        } else {
          invokeCallback(callback, null, ['p123', 'cnode', 'n127.0.0.1:3000'].join('\n'))
        }
        return { kill: vi.fn() }
      }
    )

    const scan = await scanWorkspacePorts([], { lookup: () => undefined, reconcileScan: vi.fn() })

    expect(scan.unavailableReason).toBeUndefined()
    expect(scan.ports).toHaveLength(1)
    expect(scan.ports[0]).toMatchObject({ port: 3000, pid: 123, processName: 'node' })
    expect(execFileMock.mock.calls.map(([command]) => command)).toEqual(['ss', 'lsof'])
  })

  it('does not launch fallback commands when /proc yields listeners', async () => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue('linux')
    rejectAllProcReads()
    fsPromisesMock.readFile.mockImplementation((file: unknown) =>
      file === '/proc/net/tcp'
        ? Promise.resolve(procNetTcpFixture)
        : Promise.reject(new Error('ENOENT'))
    )

    const scan = await scanWorkspacePorts([], { lookup: () => undefined, reconcileScan: vi.fn() })

    expect(scan.unavailableReason).toBeUndefined()
    expect(scan.ports.map((port) => port.port)).toEqual([3000])
    expect(execFileMock).not.toHaveBeenCalled()
  })

  it('reports an empty scan instead of unavailable when /proc is readable and tools are missing', async () => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue('linux')
    rejectAllProcReads()
    fsPromisesMock.readFile.mockResolvedValue(procNetTcpFixture.split('\n')[0])
    execFileMock.mockImplementation(
      (_command: string, _args: string[], _options: unknown, callback: unknown) => {
        invokeCallback(callback, new Error('spawn ENOENT'), '')
        return { kill: vi.fn() }
      }
    )

    const scan = await scanWorkspacePorts([], { lookup: () => undefined, reconcileScan: vi.fn() })

    expect(scan.unavailableReason).toBeUndefined()
    expect(scan.ports).toEqual([])
    expect(execFileMock.mock.calls.map(([command]) => command)).toEqual(['ss', 'lsof'])
  })
})
