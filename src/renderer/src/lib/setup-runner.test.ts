import { afterEach, describe, expect, it, vi } from 'vitest'

import { buildSetupRunnerCommand, getWorktreeSetupTerminalShellFamily } from './setup-runner'

describe('buildSetupRunnerCommand', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('uses bash with a Linux path for WSL UNC runner scripts on Windows', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
    })

    expect(
      buildSetupRunnerCommand(
        '\\\\wsl.localhost\\Ubuntu\\home\\jin\\repo\\.git\\worktrees\\feature\\orca\\setup-runner.sh'
      )
    ).toBe('bash /home/jin/repo/.git/worktrees/feature/orca/setup-runner.sh')
  })

  it('uses cmd.exe for native Windows runner scripts', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
    })

    expect(buildSetupRunnerCommand('C:\\repo\\.git\\orca\\setup-runner.cmd')).toBe(
      'cmd.exe /c "C:\\repo\\.git\\orca\\setup-runner.cmd"'
    )
  })

  it('uses bash for POSIX runner paths on Windows clients', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
    })

    expect(buildSetupRunnerCommand('/home/dev/repo/.git/orca/setup-runner.sh')).toBe(
      'bash /home/dev/repo/.git/orca/setup-runner.sh'
    )
  })

  it('uses cmd.exe for native Windows runner scripts on non-Windows clients', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'
    })

    expect(buildSetupRunnerCommand('C:\\repo\\.git\\orca\\setup-runner.cmd')).toBe(
      'cmd.exe /c "C:\\repo\\.git\\orca\\setup-runner.cmd"'
    )
  })

  it('delivers native Windows runners through Git Bash quoting when the terminal shell is posix (#6896)', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
    })

    expect(buildSetupRunnerCommand('C:\\repo\\.git\\orca\\setup-runner.cmd', 'posix')).toBe(
      `MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' cmd.exe /d /c 'C:\\repo\\.git\\orca\\setup-runner.cmd'`
    )
  })
})

describe('getWorktreeSetupTerminalShellFamily', () => {
  const state = {
    repos: [
      { id: 'repo-local', connectionId: null },
      { id: 'repo-remote', connectionId: 'conn-1' }
    ],
    worktreesByRepo: {
      'repo-local': [{ id: 'wt-local', repoId: 'repo-local' }],
      'repo-remote': [{ id: 'wt-remote', repoId: 'repo-remote' }]
    }
  }

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('classifies Git Bash as posix for local Windows worktrees', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
    })

    expect(getWorktreeSetupTerminalShellFamily(state, 'wt-local', 'git-bash')).toBe('posix')
    expect(getWorktreeSetupTerminalShellFamily(state, 'wt-local', 'cmd.exe')).toBe('cmd')
  })

  it('never overrides remote worktrees with the local Windows shell', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
    })

    expect(getWorktreeSetupTerminalShellFamily(state, 'wt-remote', 'git-bash')).toBeUndefined()
  })

  it('resolves the POSIX family off Windows clients (#8928 PR4)', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'
    })

    expect(getWorktreeSetupTerminalShellFamily(state, 'wt-local', 'git-bash')).toBe('posix')
    expect(getWorktreeSetupTerminalShellFamily(state, 'wt-local', null, 'nu')).toBe('nushell')
    // Remote worktrees still make no local-shell claim.
    expect(getWorktreeSetupTerminalShellFamily(state, 'wt-remote', null, 'nu')).toBeUndefined()
  })
})
