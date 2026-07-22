import { describe, expect, it } from 'vitest'
import { resolveLocalPosixAgentStartupShell } from './posix-terminal-shell'

describe('resolveLocalPosixAgentStartupShell (#8928 PR4)', () => {
  it('returns nushell when the local default shell setting is nu (name or path)', () => {
    expect(
      resolveLocalPosixAgentStartupShell({
        platform: 'darwin',
        isRemote: false,
        terminalPosixShell: 'nu'
      })
    ).toBe('nushell')
    expect(
      resolveLocalPosixAgentStartupShell({
        platform: 'linux',
        isRemote: false,
        terminalPosixShell: '/home/u/.cargo/bin/nu'
      })
    ).toBe('nushell')
  })

  it('returns posix for other or absent settings', () => {
    expect(
      resolveLocalPosixAgentStartupShell({
        platform: 'darwin',
        isRemote: false,
        terminalPosixShell: 'zsh'
      })
    ).toBe('posix')
    expect(
      resolveLocalPosixAgentStartupShell({
        platform: 'darwin',
        isRemote: false,
        terminalPosixShell: null
      })
    ).toBe('posix')
  })

  it('makes no claim for win32 or SSH remotes', () => {
    expect(
      resolveLocalPosixAgentStartupShell({
        platform: 'win32',
        isRemote: false,
        terminalPosixShell: 'nu'
      })
    ).toBeUndefined()
    // Why: the remote login shell kind is unknown — SSH stays 'posix' downstream (documented limitation).
    expect(
      resolveLocalPosixAgentStartupShell({
        platform: 'linux',
        isRemote: true,
        terminalPosixShell: 'nu'
      })
    ).toBeUndefined()
  })

  it('makes no claim when the launch platform is not the settings host (WSL runtime)', () => {
    expect(
      resolveLocalPosixAgentStartupShell({
        platform: 'linux',
        clientPlatform: 'win32',
        isRemote: false,
        terminalPosixShell: 'nu'
      })
    ).toBeUndefined()
    expect(
      resolveLocalPosixAgentStartupShell({
        platform: 'darwin',
        clientPlatform: 'darwin',
        isRemote: false,
        terminalPosixShell: 'nu'
      })
    ).toBe('nushell')
  })
})
