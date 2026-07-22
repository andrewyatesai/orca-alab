import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { applyShellStartupPathFiles } from './shell-startup-path'

const tempDirs: string[] = []

function createHome(): string {
  const home = mkdtempSync(join(tmpdir(), 'orca-shell-startup-path-'))
  tempDirs.push(home)
  return home
}

function makeDirectory(path: string): string {
  mkdirSync(path, { recursive: true })
  return path
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true })
  }
})

describe('applyShellStartupPathFiles', () => {
  it('adds simple PATH exports from an interactive-only zsh guard without executing it', () => {
    const home = createHome()
    const guardedBin = makeDirectory(join(home, 'company', 'bin'))
    writeFileSync(
      join(home, '.zshrc'),
      ['if [[ -o interactive ]]; then', '  export PATH="$HOME/company/bin:$PATH"', 'fi'].join('\n')
    )

    const result = applyShellStartupPathFiles('/bin/zsh', ['/usr/bin'], {
      env: { HOME: home },
      homePath: home,
      platform: 'darwin'
    })

    expect(result).toEqual({
      segments: [guardedBin, '/usr/bin'],
      changed: true
    })
  })

  it('skips command substitutions while preserving other literal PATH entries', () => {
    const home = createHome()
    const safeBin = makeDirectory(join(home, 'safe', 'bin'))
    writeFileSync(
      join(home, '.zshrc'),
      'export PATH="$(security-scanned-tool --prefix)/bin:${HOME}/safe/bin:$PATH"\n'
    )

    const result = applyShellStartupPathFiles('/bin/zsh', ['/usr/bin'], {
      env: { HOME: home },
      homePath: home,
      platform: 'darwin'
    })

    expect(result.segments).toEqual([safeBin, '/usr/bin'])
  })

  it('ignores unknown variables and nonexistent directories', () => {
    const home = createHome()
    const knownBin = makeDirectory(join(home, 'known', 'bin'))
    writeFileSync(
      join(home, '.bashrc'),
      'export PATH="$CUSTOM_BIN:$HOME/missing/bin:$HOME/known/bin:$PATH"\n'
    )

    const result = applyShellStartupPathFiles('/bin/bash', ['/usr/bin'], {
      env: { HOME: home },
      homePath: home,
      platform: 'linux'
    })

    expect(result.segments).toEqual([knownBin, '/usr/bin'])
  })

  it('applies zsh path array edits around the existing path marker', () => {
    const home = createHome()
    const firstBin = makeDirectory(join(home, 'first', 'bin'))
    const lastBin = makeDirectory(join(home, 'last', 'bin'))
    writeFileSync(join(home, '.zshrc'), `path=("$HOME/first/bin" $path "$HOME/last/bin")\n`)

    const result = applyShellStartupPathFiles('/bin/zsh', ['/usr/bin'], {
      env: { HOME: home },
      homePath: home,
      platform: 'darwin'
    })

    expect(result.segments).toEqual([firstBin, '/usr/bin', lastBin])
  })

  it('applies fish_add_path without running fish config', () => {
    const home = createHome()
    const configDir = join(home, '.config', 'fish')
    const fishBin = makeDirectory(join(home, 'fish-bin'))
    mkdirSync(configDir, { recursive: true })
    writeFileSync(
      join(configDir, 'config.fish'),
      'if status is-interactive; fish_add_path ~/fish-bin; end\n'
    )

    const result = applyShellStartupPathFiles('/usr/bin/fish', ['/usr/bin'], {
      env: { HOME: home },
      homePath: home,
      platform: 'darwin'
    })

    expect(result.segments).toEqual([fishBin, '/usr/bin'])
  })
})

describe('nu static startup scan (#8928 PR1)', () => {
  it('nu returns no static startup files (and not ~/.profile)', () => {
    const home = createHome()
    const profileBin = makeDirectory(join(home, 'profile-bin'))
    // A .profile PATH edit that WOULD be applied if nu fell into the generic default.
    writeFileSync(join(home, '.profile'), `export PATH="${profileBin}:$PATH"\n`)

    for (const shell of ['nu', '/usr/local/bin/nu', '/home/u/.cargo/bin/nu-0.104', 'nu.exe']) {
      const result = applyShellStartupPathFiles(shell, ['/usr/bin'], {
        env: { HOME: home },
        homePath: home,
        platform: 'darwin'
      })
      // Why: nu never reads ~/.profile — mirroring it would surface segments the live shell doesn't have.
      expect(result.segments).toEqual(['/usr/bin'])
      expect(result.changed).toBe(false)
    }
  })

  it('unknown shells still fall back to ~/.profile', () => {
    const home = createHome()
    const profileBin = makeDirectory(join(home, 'profile-bin'))
    writeFileSync(join(home, '.profile'), `export PATH="${profileBin}:$PATH"\n`)

    const result = applyShellStartupPathFiles('/opt/weird/xonsh', ['/usr/bin'], {
      env: { HOME: home },
      homePath: home,
      platform: 'darwin'
    })
    expect(result.segments).toEqual([profileBin, '/usr/bin'])
  })
})
