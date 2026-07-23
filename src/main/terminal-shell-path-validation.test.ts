import { describe, expect, it } from 'vitest'
import { validateTerminalShellPath } from './terminal-shell-path-validation'

const FILE = { isFile: true, isDirectory: false }
const DIRECTORY = { isFile: false, isDirectory: true }

describe('validateTerminalShellPath (win32)', () => {
  const win = { platform: 'win32' as const }

  it('accepts an absolute executable path and returns it normalized', () => {
    expect(
      validateTerminalShellPath('D:\\tools\\pwsh-daily\\pwsh.exe', {
        ...win,
        statPath: () => FILE
      })
    ).toEqual({ ok: true, resolvedPath: 'D:\\tools\\pwsh-daily\\pwsh.exe' })
  })

  it('accepts cmd/bat/com launchers', () => {
    for (const name of ['shell.cmd', 'shell.bat', 'shell.com']) {
      expect(
        validateTerminalShellPath(`C:\\shells\\${name}`, { ...win, statPath: () => FILE }).ok
      ).toBe(true)
    }
  })

  it('rejects relative, missing, directory, and non-executable-extension paths', () => {
    expect(validateTerminalShellPath('tools\\pwsh.exe', win)).toEqual({
      ok: false,
      reason: 'not-absolute'
    })
    expect(validateTerminalShellPath('', win)).toEqual({ ok: false, reason: 'not-absolute' })
    expect(
      validateTerminalShellPath('D:\\gone\\pwsh.exe', { ...win, statPath: () => null })
    ).toEqual({ ok: false, reason: 'not-found' })
    expect(
      validateTerminalShellPath('D:\\tools', { ...win, statPath: () => DIRECTORY })
    ).toEqual({ ok: false, reason: 'is-directory' })
    expect(
      validateTerminalShellPath('D:\\tools\\shell.ps1', { ...win, statPath: () => FILE })
    ).toEqual({ ok: false, reason: 'not-executable' })
  })

  it('reports a Store App Execution Alias as not-executable with its recoverable target', () => {
    const alias = 'C:\\Users\\dev\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe'
    const target = 'C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7\\pwsh.exe'
    expect(
      validateTerminalShellPath(alias, {
        ...win,
        statPath: () => FILE,
        resolveAppExecutionAlias: () => target
      })
    ).toEqual({ ok: false, reason: 'not-executable', resolvedPath: target })
  })

  it('omits the alias target when it is unreadable or itself an alias', () => {
    const alias = 'C:\\Users\\dev\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe'
    expect(
      validateTerminalShellPath(alias, {
        ...win,
        statPath: () => FILE,
        resolveAppExecutionAlias: () => null
      })
    ).toEqual({ ok: false, reason: 'not-executable' })
    expect(
      validateTerminalShellPath(alias, {
        ...win,
        statPath: () => FILE,
        resolveAppExecutionAlias: () => alias
      })
    ).toEqual({ ok: false, reason: 'not-executable' })
  })
})

describe('validateTerminalShellPath (posix)', () => {
  const posix = { platform: 'darwin' as const }

  it('accepts an absolute executable path', () => {
    expect(
      validateTerminalShellPath('/usr/local/bin/fish', {
        ...posix,
        statPath: () => FILE,
        isExecutable: () => true
      })
    ).toEqual({ ok: true, resolvedPath: '/usr/local/bin/fish' })
  })

  it('rejects relative, missing, directory, and non-executable paths', () => {
    expect(validateTerminalShellPath('bin/fish', posix)).toEqual({
      ok: false,
      reason: 'not-absolute'
    })
    expect(
      validateTerminalShellPath('/gone/fish', { ...posix, statPath: () => null })
    ).toEqual({ ok: false, reason: 'not-found' })
    expect(
      validateTerminalShellPath('/usr/local/bin', { ...posix, statPath: () => DIRECTORY })
    ).toEqual({ ok: false, reason: 'is-directory' })
    expect(
      validateTerminalShellPath('/etc/shells', {
        ...posix,
        statPath: () => FILE,
        isExecutable: () => false
      })
    ).toEqual({ ok: false, reason: 'not-executable' })
  })

  it('trims surrounding whitespace before validating', () => {
    expect(
      validateTerminalShellPath('  /bin/zsh  ', {
        ...posix,
        statPath: () => FILE,
        isExecutable: () => true
      })
    ).toEqual({ ok: true, resolvedPath: '/bin/zsh' })
  })
})
