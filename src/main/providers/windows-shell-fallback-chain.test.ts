import { afterEach, describe, expect, it } from 'vitest'
import { win32 as pathWin32 } from 'node:path'
import { buildWindowsPowerShellSpawnAttempts } from './windows-shell-fallback-chain'

const WIN_ENV: NodeJS.ProcessEnv = {
  ProgramW6432: 'C:\\Program Files',
  SystemRoot: 'C:\\Windows',
  ComSpec: 'C:\\Windows\\System32\\cmd.exe'
}

const PWSH7 = 'C:\\Program Files\\PowerShell\\7\\pwsh.exe'
const WINDOWS_POWERSHELL = 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe'
const CMD = 'C:\\Windows\\System32\\cmd.exe'

function setPlatform(platform: NodeJS.Platform): () => void {
  const original = process.platform
  Object.defineProperty(process, 'platform', { configurable: true, value: platform })
  return () => Object.defineProperty(process, 'platform', { configurable: true, value: original })
}

let restorePlatform: (() => void) | null = null
afterEach(() => {
  restorePlatform?.()
  restorePlatform = null
})

describe('buildWindowsPowerShellSpawnAttempts', () => {
  it('returns no attempts for non-PowerShell shells (cmd.exe keeps single-shell behavior)', () => {
    restorePlatform = setPlatform('win32')
    expect(
      buildWindowsPowerShellSpawnAttempts({
        shellPath: 'cmd.exe',
        cwd: 'C:\\repo',
        defaultCwd: 'C:\\Users\\dev'
      })
    ).toEqual([])
  })

  it('builds pwsh -> Windows PowerShell -> cmd.exe attempts with per-shell args', () => {
    restorePlatform = setPlatform('win32')
    const attempts = buildWindowsPowerShellSpawnAttempts({
      shellPath: 'pwsh.exe',
      cwd: 'C:\\repo',
      defaultCwd: 'C:\\Users\\dev',
      resolveOptions: {
        platform: 'win32',
        env: WIN_ENV,
        isRealExecutable: (p) => p === PWSH7 || p === WINDOWS_POWERSHELL
      }
    })
    expect(attempts.map((a) => a.shellPath)).toEqual([PWSH7, WINDOWS_POWERSHELL, CMD])
    // PowerShell links use -EncodedCommand; cmd.exe uses /K chcp.
    expect(attempts[0].shellArgs).toContain('-EncodedCommand')
    expect(attempts[1].shellArgs).toContain('-EncodedCommand')
    expect(attempts[2].shellArgs[0]).toBe('/K')
  })

  it('keeps an absolute custom shellPath as attempt 0, not replaced by the discovered install (#7467)', () => {
    restorePlatform = setPlatform('win32')
    const customPwsh = 'D:\\tools\\pwsh-daily\\pwsh.exe'
    const attempts = buildWindowsPowerShellSpawnAttempts({
      shellPath: customPwsh,
      cwd: 'C:\\repo',
      defaultCwd: 'C:\\Users\\dev',
      resolveOptions: {
        platform: 'win32',
        env: WIN_ENV,
        isRealExecutable: (p) => p === customPwsh || p === PWSH7 || p === WINDOWS_POWERSHELL
      }
    })
    expect(attempts.map((a) => a.shellPath)).toEqual([customPwsh, PWSH7, WINDOWS_POWERSHELL, CMD])
    expect(attempts[0].shellArgs).toContain('-EncodedCommand')
  })

  it('de-dups a custom path that equals a discovered chain link (#7467)', () => {
    restorePlatform = setPlatform('win32')
    const attempts = buildWindowsPowerShellSpawnAttempts({
      // Case-insensitive match against the discovered pwsh install must not double it.
      shellPath: 'c:\\program files\\powershell\\7\\PWSH.EXE',
      cwd: 'C:\\repo',
      defaultCwd: 'C:\\Users\\dev',
      resolveOptions: {
        platform: 'win32',
        env: WIN_ENV,
        isRealExecutable: (p) => p.toLowerCase() === PWSH7.toLowerCase() || p === WINDOWS_POWERSHELL
      }
    })
    expect(attempts.map((a) => a.shellPath.toLowerCase())).toEqual(
      [PWSH7, WINDOWS_POWERSHELL, CMD].map((p) => p.toLowerCase())
    )
  })

  it('drops a non-executable absolute custom path to the discovered chain (#7467)', () => {
    restorePlatform = setPlatform('win32')
    const attempts = buildWindowsPowerShellSpawnAttempts({
      shellPath: 'D:\\removed\\pwsh.exe',
      cwd: 'C:\\repo',
      defaultCwd: 'C:\\Users\\dev',
      resolveOptions: {
        platform: 'win32',
        env: WIN_ENV,
        isRealExecutable: (p) => p === PWSH7 || p === WINDOWS_POWERSHELL
      }
    })
    expect(attempts.map((a) => a.shellPath)).toEqual([PWSH7, WINDOWS_POWERSHELL, CMD])
  })

  it('never promotes an absolute Store-alias custom path to attempt 0 (#7467)', () => {
    restorePlatform = setPlatform('win32')
    const aliasStub = 'C:\\Users\\dev\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe'
    const attempts = buildWindowsPowerShellSpawnAttempts({
      shellPath: aliasStub,
      cwd: 'C:\\repo',
      defaultCwd: 'C:\\Users\\dev',
      resolveOptions: {
        platform: 'win32',
        env: WIN_ENV,
        // Even a permissive probe must not resurrect the alias — ConPTY cannot launch it.
        isRealExecutable: (p) => p === aliasStub || p === PWSH7 || p === WINDOWS_POWERSHELL
      }
    })
    expect(attempts.map((a) => a.shellPath)).toEqual([PWSH7, WINDOWS_POWERSHELL, CMD])
  })

  it('repro: when pwsh is only a Store alias, the primary attempt is the real Windows PowerShell', () => {
    restorePlatform = setPlatform('win32')
    const aliasStub = 'C:\\Users\\dev\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe'
    const attempts = buildWindowsPowerShellSpawnAttempts({
      shellPath: 'pwsh.exe',
      cwd: 'C:\\repo',
      defaultCwd: 'C:\\Users\\dev',
      resolveOptions: {
        platform: 'win32',
        env: WIN_ENV,
        isRealExecutable: (p) => p === aliasStub || p === WINDOWS_POWERSHELL
      }
    })
    // The bare/alias pwsh.exe must never be the primary spawn target.
    expect(attempts[0].shellPath).toBe(WINDOWS_POWERSHELL)
    expect(attempts.map((a) => a.shellPath)).not.toContain(aliasStub)
    expect(attempts.map((a) => a.shellPath)).not.toContain('pwsh.exe')
    // Every attempt is an absolute path ConPTY can launch.
    for (const attempt of attempts) {
      expect(pathWin32.isAbsolute(attempt.shellPath)).toBe(true)
    }
  })
})
