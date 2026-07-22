import { describe, expect, it } from 'vitest'
import {
  buildSetupRunnerCommand,
  getSetupRunnerCommandPlatformForPath
} from './setup-runner-command'

describe('buildSetupRunnerCommand', () => {
  it('uses bash for WSL UNC runner scripts regardless of host casing', () => {
    expect(
      buildSetupRunnerCommand(
        '\\\\WSL.LOCALHOST\\Ubuntu\\home\\jin\\repo\\.git\\worktrees\\feature\\orca\\setup-runner.sh',
        'windows'
      )
    ).toBe('bash /home/jin/repo/.git/worktrees/feature/orca/setup-runner.sh')
  })

  it('uses bash with Linux paths for forward-slash WSL UNC runner scripts', () => {
    expect(
      buildSetupRunnerCommand(
        '//wsl.localhost/Ubuntu/home/jin/repo/.git/worktrees/feature/orca/setup-runner.sh',
        'windows'
      )
    ).toBe('bash /home/jin/repo/.git/worktrees/feature/orca/setup-runner.sh')
  })

  it('keeps generic forward-slash UNC runner scripts on cmd.exe', () => {
    expect(
      buildSetupRunnerCommand('//server/share/repo/.git/orca/setup-runner.cmd', 'windows')
    ).toBe('cmd.exe /c "//server/share/repo/.git/orca/setup-runner.cmd"')
  })

  it('delivers native Windows runners through POSIX quoting for Git Bash terminals (#6896)', () => {
    expect(
      buildSetupRunnerCommand('C:\\repo\\.git\\orca\\setup-runner.cmd', 'windows', 'posix')
    ).toBe(
      `MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' cmd.exe /d /c 'C:\\repo\\.git\\orca\\setup-runner.cmd'`
    )
  })

  it('nu-escapes the .cmd path for Nushell terminals (#8928 PR4)', () => {
    // Why: nu double-quoted strings treat \ as an escape; unescaped C:\… errors when typed into nu.
    expect(
      buildSetupRunnerCommand('C:\\repo\\.git\\orca\\setup-runner.cmd', 'windows', 'nushell')
    ).toBe('cmd.exe /c "C:\\\\repo\\\\.git\\\\orca\\\\setup-runner.cmd"')
  })

  it('keeps cmd.exe delivery for cmd and PowerShell terminals', () => {
    expect(buildSetupRunnerCommand('C:\\repo\\.git\\orca\\setup-runner.cmd', 'windows', 'cmd')).toBe(
      'cmd.exe /c "C:\\repo\\.git\\orca\\setup-runner.cmd"'
    )
    expect(
      buildSetupRunnerCommand('C:\\repo\\.git\\orca\\setup-runner.cmd', 'windows', 'powershell')
    ).toBe('cmd.exe /c "C:\\repo\\.git\\orca\\setup-runner.cmd"')
  })

  it('keeps bash delivery for WSL UNC runners regardless of terminal shell family', () => {
    expect(
      buildSetupRunnerCommand(
        '\\\\wsl.localhost\\Ubuntu\\home\\jin\\repo\\.git\\orca\\setup-runner.sh',
        'windows',
        'posix'
      )
    ).toBe('bash /home/jin/repo/.git/orca/setup-runner.sh')
  })
})

describe('getSetupRunnerCommandPlatformForPath', () => {
  it('prefers POSIX for absolute POSIX runner paths even from Windows clients', () => {
    expect(
      getSetupRunnerCommandPlatformForPath('/remote/repo/.git/orca/setup-runner.sh', 'windows')
    ).toBe('posix')
  })

  it('prefers Windows for native Windows runner paths even from POSIX clients', () => {
    expect(
      getSetupRunnerCommandPlatformForPath('C:\\repo\\.git\\orca\\setup-runner.cmd', 'posix')
    ).toBe('windows')
  })

  it('keeps WSL UNC paths on the Windows resolver so they can be converted', () => {
    expect(
      getSetupRunnerCommandPlatformForPath(
        '\\\\wsl.localhost\\Ubuntu\\home\\jin\\repo\\.git\\orca\\setup-runner.sh',
        'posix'
      )
    ).toBe('windows')
  })

  it('keeps forward-slash UNC paths on the Windows resolver', () => {
    expect(
      getSetupRunnerCommandPlatformForPath(
        '//wsl.localhost/Ubuntu/home/jin/repo/.git/orca/setup-runner.sh',
        'posix'
      )
    ).toBe('windows')
    expect(
      getSetupRunnerCommandPlatformForPath(
        '//server/share/repo/.git/orca/setup-runner.cmd',
        'posix'
      )
    ).toBe('windows')
  })
})
