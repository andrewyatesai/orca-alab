/**
 * End-to-end deploy coverage for the node-pty patch delivery (#8855/#9586):
 * a fresh install must overwrite the vanilla remote node-pty runtime files
 * with the patched payload bundled in the local relay package, after the
 * remote `npm install` so the registry files cannot win.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getAppPath: () => '/mock/app' }
}))

vi.mock('fs', () => ({
  existsSync: vi.fn().mockReturnValue(true),
  readFileSync: vi.fn().mockReturnValue('0.1.0+abcdef012345'),
  readdirSync: vi.fn().mockReturnValue([])
}))

vi.mock('./relay-protocol', () => ({
  RELAY_VERSION: '0.1.0',
  RELAY_REMOTE_DIR: '.orca-remote',
  parseUnameToRelayPlatform: vi.fn(() => 'linux-x64'),
  RELAY_SENTINEL: 'ORCA-RELAY v0.1.0 READY\n',
  RELAY_SENTINEL_TIMEOUT_MS: 10_000
}))

vi.mock('./ssh-relay-deploy-helpers', () => ({
  uploadDirectory: vi.fn().mockResolvedValue(undefined),
  waitForSentinel: vi.fn().mockResolvedValue({
    write: vi.fn(),
    onData: vi.fn(),
    onClose: vi.fn()
  }),
  isUnconfirmedSshCommandTermination: () => false,
  execCommand: vi.fn().mockResolvedValue('__ORCA_REMOTE_PLATFORM__ Linux x86_64')
}))

vi.mock('./ssh-remote-node-resolution', () => ({
  resolveRemoteNodePath: vi.fn().mockResolvedValue('/usr/bin/node')
}))

vi.mock('./ssh-relay-versioned-install', () => ({
  readLocalFullVersion: vi.fn().mockReturnValue('0.1.0+abcdef012345'),
  computeRemoteRelayDir: (home: string, v: string) => `${home}/.orca-remote/relay-${v}`,
  isRelayAlreadyInstalled: vi.fn().mockResolvedValue(false),
  finalizeInstall: vi.fn().mockResolvedValue(undefined),
  abandonInstall: vi.fn().mockResolvedValue(undefined),
  gcOldRelayVersions: vi.fn().mockResolvedValue(undefined)
}))

vi.mock('./ssh-relay-install-lock', () => ({
  acquireInstallLock: vi.fn().mockResolvedValue(undefined)
}))

vi.mock('./ssh-relay-repair-lock', () => ({
  tryAcquireRelayRepairLock: vi.fn().mockResolvedValue('acquired')
}))

vi.mock('./ssh-connection-utils', () => ({
  shellEscape: (s: string) => `'${s}'`,
  createSshOperationAbortError: () =>
    Object.assign(new Error('SSH operation was cancelled'), { name: 'AbortError' })
}))

import { readdirSync } from 'node:fs'
import { deployAndLaunchRelay } from './ssh-relay-deploy'
import { execCommand } from './ssh-relay-deploy-helpers'
import type { SshConnection } from './ssh-connection'

function makeInstallCapableConnection(): SshConnection {
  return {
    canRunConcurrentExecCommands: vi.fn().mockReturnValue(true),
    exec: vi.fn().mockResolvedValue({
      on: vi.fn(),
      stderr: { on: vi.fn() },
      stdin: {},
      stdout: { on: vi.fn() },
      close: vi.fn()
    }),
    uploadDirectory: vi.fn().mockResolvedValue(undefined),
    writeFile: vi.fn().mockResolvedValue(undefined)
  } as unknown as SshConnection
}

describe('deployAndLaunchRelay node-pty patch delivery', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('overwrites the remote node-pty install with the local patched payload after npm install', async () => {
    const conn = makeInstallCapableConnection()
    const mockExecCommand = vi.mocked(execCommand)
    mockExecCommand.mockResolvedValueOnce('__ORCA_REMOTE_PLATFORM__ Linux x86_64') // platform probe
    mockExecCommand.mockResolvedValueOnce('/home/user') // echo $HOME
    mockExecCommand.mockResolvedValueOnce('') // mkdir remote relay dir
    mockExecCommand.mockResolvedValueOnce('') // chmod +x node
    mockExecCommand.mockResolvedValueOnce('') // npm install
    mockExecCommand.mockResolvedValueOnce('') // chmod spawn-helper prebuilds
    mockExecCommand.mockResolvedValueOnce('ORCA-NPTY-PROBE-OK') // post-install probe
    mockExecCommand.mockResolvedValueOnce('') // rm probe stderr
    mockExecCommand.mockResolvedValueOnce('DEAD') // socket probe
    mockExecCommand.mockResolvedValueOnce('READY') // socket poll
    const dirent = (name: string, directory: boolean) => ({
      name,
      isDirectory: () => directory,
      isFile: () => !directory
    })
    vi.mocked(readdirSync).mockImplementation(((dir: unknown) => {
      const path = String(dir)
      if (path.endsWith('node-pty-patched')) {
        return [dirent('lib', true)]
      }
      if (path.endsWith('lib')) {
        return [dirent('conpty_console_list_agent.js', false), dirent('unixTerminal.js', false)]
      }
      return []
    }) as never)

    await deployAndLaunchRelay(conn)

    const writtenPaths = vi.mocked(conn.writeFile!).mock.calls.map(([p]) => p as string)
    expect(writtenPaths).toContain(
      '/home/user/.orca-remote/relay-0.1.0+abcdef012345/node_modules/node-pty/lib/conpty_console_list_agent.js'
    )
    expect(writtenPaths).toContain(
      '/home/user/.orca-remote/relay-0.1.0+abcdef012345/node_modules/node-pty/lib/unixTerminal.js'
    )
    // Delivery must land after npm install so the vanilla files cannot win.
    const npmInstallIndex = mockExecCommand.mock.calls.findIndex(([, c]) =>
      c.includes('npm install')
    )
    expect(npmInstallIndex).toBeGreaterThanOrEqual(0)
    const npmInstallOrder = mockExecCommand.mock.invocationCallOrder[npmInstallIndex]
    const deliveryIndex = writtenPaths.findIndex((p) => p.includes('node_modules/node-pty'))
    const deliveryOrder = vi.mocked(conn.writeFile!).mock.invocationCallOrder[deliveryIndex]
    expect(deliveryOrder).toBeGreaterThan(npmInstallOrder)
  })
})
