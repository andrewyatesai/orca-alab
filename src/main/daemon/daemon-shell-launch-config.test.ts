// The client-precomputed POSIX launch config the adapter ships to the Rust
// daemon (audit F4b/F4d): login-shell default, ZDOTDIR/rcfile wrapper args +
// env, codex markerless attribution mode, and the macOS login(1) TCC wrap.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { mkdtempSync, rmSync } from 'node:fs'
import { buildPosixDaemonShellLaunch } from './daemon-shell-launch-config'
import {
  prepareMacosTccLoginShell,
  resetMacosLoginShellPreflightForTests
} from '../providers/macos-tcc-login-shell'

const { getUserDataPathMock, execFileMock } = vi.hoisted(() => ({
  getUserDataPathMock: vi.fn<() => string>(),
  execFileMock: vi.fn()
}))

// Why: upstream gates the login(1) wrap behind a PAM preflight subprocess; stub
// it so the dedicated TCC test below can prime the wrap deterministically.
vi.mock('node:child_process', () => ({ execFile: execFileMock }))

vi.mock('electron', () => ({
  app: {
    getPath: (name: string) => {
      if (name === 'userData') {
        return getUserDataPathMock()
      }
      throw new Error(`unexpected app.getPath(${name})`)
    }
  }
}))

const posixDescribe = process.platform === 'win32' ? describe.skip : describe

posixDescribe('buildPosixDaemonShellLaunch (POSIX)', () => {
  let userDataDir: string
  const savedDisable = process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL

  beforeEach(() => {
    userDataDir = mkdtempSync(join(tmpdir(), 'orca-launch-config-'))
    getUserDataPathMock.mockReturnValue(userDataDir)
    // Why: keep program/args deterministic across dev machines — the login(1)
    // wrap has its own dedicated test below.
    process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL = '1'
  })

  afterEach(() => {
    resetMacosLoginShellPreflightForTests()
    rmSync(userDataDir, { recursive: true, force: true })
    if (savedDisable === undefined) {
      delete process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL
    } else {
      process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL = savedDisable
    }
  })

  it('plain sessions default to a login shell with no wrapper env', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: { FOO: 'bar' }
    })
    expect(launch).not.toBeNull()
    expect(launch!.shellOverride).toBe('/bin/zsh')
    expect(launch!.shellArgs).toEqual(['-l'])
    expect(launch!.env).toEqual({
      FOO: 'bar',
      POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD: 'true'
    })
  })

  it('seeds the p10k wizard guard on command sessions (Node-daemon parity)', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: {},
      command: 'pnpm dev'
    })
    expect(launch!.env?.POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD).toBe('true')
  })

  it('keeps a user-provided p10k wizard value over the seed', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: { POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD: 'false' }
    })
    expect(launch!.env?.POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD).toBe('false')
  })

  it('skips the p10k seed when the launch mode deletes the var', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: {},
      envToDelete: ['POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD']
    })
    expect(launch!.env?.POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD).toBeUndefined()
  })

  it('zsh command sessions get the marker wrapper: -l + ZDOTDIR env', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: {},
      command: 'pnpm dev'
    })
    expect(launch!.shellArgs).toEqual(['-l'])
    expect(launch!.env?.ZDOTDIR?.endsWith(join('shell-ready', 'zsh'))).toBe(true)
    expect(launch!.env?.ORCA_SHELL_READY_MARKER).toBe('1')
  })

  it('bash command sessions get the --rcfile wrapper', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/bash',
      env: {},
      command: 'pnpm dev'
    })
    expect(launch!.shellArgs[0]).toBe('--rcfile')
    expect(launch!.shellArgs[1]?.endsWith(join('bash', 'rcfile'))).toBe(true)
    expect(launch!.env?.ORCA_SHELL_READY_MARKER).toBe('1')
  })

  it('markerless codex keeps the attribution wrapper (no ready marker)', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: {},
      command: 'codex'
    })
    expect(launch!.env?.ORCA_SHELL_READY_MARKER).toBe('0')
  })

  it('payload-bearing codex waits for the marker wrapper', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: {},
      command: 'codex --prefill "fix the tests"'
    })
    expect(launch!.env?.ORCA_SHELL_READY_MARKER).toBe('1')
  })

  it('launch-mode env without a command still gets the attribution wrapper', () => {
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: { ORCA_CODEX_HOME: '/tmp/codex-home' }
    })
    expect(launch!.env?.ORCA_SHELL_READY_MARKER).toBe('0')
    expect(launch!.env?.ZDOTDIR?.endsWith(join('shell-ready', 'zsh'))).toBe(true)
  })

  it('resolves the shell from env.SHELL when no override is given', () => {
    const launch = buildPosixDaemonShellLaunch({
      env: { SHELL: '/bin/bash' },
      command: 'pnpm dev'
    })
    expect(launch!.shellOverride).toBe('/bin/bash')
    expect(launch!.shellArgs[0]).toBe('--rcfile')
  })

  const darwinIt = process.platform === 'darwin' ? it : it.skip
  darwinIt('wraps the spawn in login(1) for TCC attribution on macOS', async () => {
    delete process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL
    // Prime the PAM preflight cache (upstream gate): the wrap only engages after
    // prepareMacosTccLoginShell observed a clean login(1) probe.
    execFileMock.mockImplementation((_file, _args, _options, callback) => {
      callback(null, 'ORCA_LOGIN_PREFLIGHT_OK', '')
      return { stdin: { end: vi.fn() } }
    })
    resetMacosLoginShellPreflightForTests()
    await prepareMacosTccLoginShell()
    const launch = buildPosixDaemonShellLaunch({
      shellOverride: '/bin/zsh',
      env: {}
    })
    expect(launch!.shellOverride).toBe('/usr/bin/login')
    expect(launch!.shellArgs[0]).toBe('-flpq')
    expect(launch!.shellArgs).toContain('/bin/zsh')
    expect(launch!.shellArgs.at(-1)).toBe('-l')
  })
})

describe('buildPosixDaemonShellLaunch (Windows)', () => {
  it('returns null on win32 — the Node daemon owns that launch layer', () => {
    const original = Object.getOwnPropertyDescriptor(process, 'platform')!
    Object.defineProperty(process, 'platform', { value: 'win32' })
    try {
      expect(buildPosixDaemonShellLaunch({ shellOverride: 'wsl.exe' })).toBeNull()
    } finally {
      Object.defineProperty(process, 'platform', original)
    }
  })
})
