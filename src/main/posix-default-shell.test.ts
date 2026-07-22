import { describe, expect, it } from 'vitest'
import {
  detectPosixTerminalShells,
  resolveLocalPosixShellOverride,
  resolvePosixShellSettingPath,
  type PosixShellProbeOptions
} from './posix-default-shell'

function probe(overrides: Partial<PosixShellProbeOptions> = {}): PosixShellProbeOptions {
  return {
    platform: 'darwin',
    env: { PATH: '/usr/bin:/bin', SHELL: '/bin/zsh' },
    isExecutable: () => false,
    readEtcShells: () => '',
    ...overrides
  }
}

describe('resolvePosixShellSettingPath', () => {
  it('returns null for unset or blank settings', () => {
    expect(resolvePosixShellSettingPath(undefined, probe())).toBeNull()
    expect(resolvePosixShellSettingPath(null, probe())).toBeNull()
    expect(resolvePosixShellSettingPath('   ', probe())).toBeNull()
  })

  it('returns null on Windows regardless of the value', () => {
    expect(
      resolvePosixShellSettingPath('zsh', probe({ platform: 'win32', isExecutable: () => true }))
    ).toBeNull()
  })

  it('resolves a bare name through /etc/shells entries first', () => {
    const path = resolvePosixShellSettingPath(
      'fish',
      probe({
        readEtcShells: () => '# login shells\n/bin/bash\n/opt/homebrew/bin/fish\n',
        isExecutable: (candidate) => candidate === '/opt/homebrew/bin/fish'
      })
    )
    expect(path).toBe('/opt/homebrew/bin/fish')
  })

  it('falls back to PATH directories, skipping relative segments', () => {
    const checked: string[] = []
    const path = resolvePosixShellSettingPath(
      'zsh',
      probe({
        env: { PATH: 'relative/dir:/usr/local/bin' },
        isExecutable: (candidate) => {
          checked.push(candidate)
          return candidate === '/usr/local/bin/zsh'
        }
      })
    )
    expect(path).toBe('/usr/local/bin/zsh')
    expect(checked).not.toContain('relative/dir/zsh')
  })

  it('falls back to the static shell directories', () => {
    const path = resolvePosixShellSettingPath(
      'bash',
      probe({ env: {}, isExecutable: (candidate) => candidate === '/bin/bash' })
    )
    expect(path).toBe('/bin/bash')
  })

  it('validates an explicit path without candidate search', () => {
    const opts = probe({ isExecutable: (candidate) => candidate === '/opt/weird/xonsh' })
    expect(resolvePosixShellSettingPath('/opt/weird/xonsh', opts)).toBe('/opt/weird/xonsh')
    expect(resolvePosixShellSettingPath('/missing/fish', opts)).toBeNull()
  })
})

describe('detectPosixTerminalShells', () => {
  it('lists only installed shells with resolved paths', () => {
    const detection = detectPosixTerminalShells(
      probe({
        env: { PATH: '/usr/bin:/bin', SHELL: '/usr/bin/zsh' },
        isExecutable: (candidate) => candidate === '/bin/zsh' || candidate === '/bin/bash'
      })
    )
    expect(detection.shells).toEqual([
      { shell: 'zsh', path: '/bin/zsh' },
      { shell: 'bash', path: '/bin/bash' }
    ])
    expect(detection.systemShellName).toBe('zsh')
  })

  it('reports an empty catalog on Windows', () => {
    expect(detectPosixTerminalShells(probe({ platform: 'win32' }))).toEqual({
      shells: [],
      systemShellName: null
    })
  })

  it('reports a null system shell when SHELL is unset', () => {
    const detection = detectPosixTerminalShells(probe({ env: {} }))
    expect(detection.systemShellName).toBeNull()
  })
})

describe('resolveLocalPosixShellOverride', () => {
  it('keeps an explicit per-tab override without resolving the setting', () => {
    expect(resolveLocalPosixShellOverride('/bin/bash', 'fish', probe())).toBe('/bin/bash')
  })

  it('resolves the global setting when no override is requested', () => {
    const override = resolveLocalPosixShellOverride(
      undefined,
      'fish',
      probe({ isExecutable: (candidate) => candidate === '/usr/bin/fish' })
    )
    expect(override).toBe('/usr/bin/fish')
  })

  it('returns undefined when the setting cannot be resolved', () => {
    expect(resolveLocalPosixShellOverride(undefined, 'fish', probe())).toBeUndefined()
    expect(resolveLocalPosixShellOverride(undefined, null, probe())).toBeUndefined()
  })
})

describe('nu detection (#8928 PR1)', () => {
  it('detects nu from PATH and ~/.cargo/bin candidates', () => {
    const fromPath = detectPosixTerminalShells(
      probe({
        env: { PATH: '/usr/local/bin:/usr/bin', SHELL: '/bin/zsh' },
        isExecutable: (candidate) => candidate === '/usr/local/bin/nu'
      })
    )
    expect(fromPath.shells).toContainEqual({ shell: 'nu', path: '/usr/local/bin/nu' })

    // Why: cargo installs live outside PATH for GUI-launched Electron; the HOME-relative candidates cover them.
    const fromCargo = detectPosixTerminalShells(
      probe({
        env: { PATH: '/usr/bin', HOME: '/home/tester', SHELL: '/bin/zsh' },
        isExecutable: (candidate) => candidate === '/home/tester/.cargo/bin/nu'
      })
    )
    expect(fromCargo.shells).toContainEqual({ shell: 'nu', path: '/home/tester/.cargo/bin/nu' })

    const fromLocalBin = detectPosixTerminalShells(
      probe({
        env: { PATH: '/usr/bin', HOME: '/home/tester', SHELL: '/bin/zsh' },
        isExecutable: (candidate) => candidate === '/home/tester/.local/bin/nu'
      })
    )
    expect(fromLocalBin.shells).toContainEqual({ shell: 'nu', path: '/home/tester/.local/bin/nu' })
  })

  it('ignores a relative HOME for candidate construction', () => {
    const checked: string[] = []
    resolvePosixShellSettingPath(
      'nu',
      probe({
        env: { PATH: '/usr/bin', HOME: 'relative-home' },
        isExecutable: (candidate) => {
          checked.push(candidate)
          return false
        }
      })
    )
    expect(checked.some((candidate) => candidate.startsWith('relative-home'))).toBe(false)
  })

  it('nu choice resolves an explicit path setting', () => {
    const opts = probe({ isExecutable: (candidate) => candidate === '/opt/nu/bin/nu' })
    expect(resolvePosixShellSettingPath('/opt/nu/bin/nu', opts)).toBe('/opt/nu/bin/nu')
    expect(
      resolveLocalPosixShellOverride(
        undefined,
        'nu',
        probe({ isExecutable: (candidate) => candidate === '/usr/local/bin/nu' })
      )
    ).toBe('/usr/local/bin/nu')
  })
})
