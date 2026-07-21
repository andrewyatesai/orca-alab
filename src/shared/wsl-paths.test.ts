import { describe, expect, it } from 'vitest'
import {
  foldWslUncPathCaseInsensitiveParts,
  isWslUncPath,
  mapPosixPathToWslWorktreeUncPath,
  parseWslUncPath
} from './wsl-paths'

describe('wsl path helpers', () => {
  it('parses modern and legacy WSL UNC paths without platform checks', () => {
    expect(parseWslUncPath('\\\\wsl.localhost\\Ubuntu\\home\\jin\\repo')).toEqual({
      distro: 'Ubuntu',
      linuxPath: '/home/jin/repo'
    })
    expect(parseWslUncPath('\\\\wsl$\\Debian\\home\\jin')).toEqual({
      distro: 'Debian',
      linuxPath: '/home/jin'
    })
  })

  it('rejects ordinary Windows and POSIX paths', () => {
    expect(isWslUncPath('C:\\Users\\jin\\repo')).toBe(false)
    expect(isWslUncPath('/home/jin/repo')).toBe(false)
  })
})

describe('foldWslUncPathCaseInsensitiveParts', () => {
  it('folds share spelling, distro casing, and separators but not the Linux tail', () => {
    expect(foldWslUncPathCaseInsensitiveParts('\\\\WSL$\\Ubuntu\\home\\jin\\Repo')).toBe(
      '//wsl.localhost/ubuntu/home/jin/Repo'
    )
    expect(foldWslUncPathCaseInsensitiveParts('//wsl.localhost/UBUNTU/home/jin/Repo')).toBe(
      '//wsl.localhost/ubuntu/home/jin/Repo'
    )
  })

  it('folds drvfs automount tails but not other /mnt entries', () => {
    expect(foldWslUncPathCaseInsensitiveParts('\\\\wsl$\\Ubuntu\\mnt\\C\\Users\\Jin')).toBe(
      '//wsl.localhost/ubuntu/mnt/c/users/jin'
    )
    expect(foldWslUncPathCaseInsensitiveParts('\\\\wsl$\\Ubuntu\\mnt\\wsl\\Data')).toBe(
      '//wsl.localhost/ubuntu/mnt/wsl/Data'
    )
  })

  it('does not treat a case-variant /MNT dir as the drvfs automount', () => {
    expect(foldWslUncPathCaseInsensitiveParts('\\\\wsl$\\Ubuntu\\MNT\\c\\Repo')).toBe(
      '//wsl.localhost/ubuntu/MNT/c/Repo'
    )
  })

  it('returns null for non-WSL paths', () => {
    expect(foldWslUncPathCaseInsensitiveParts('C:\\Users\\jin')).toBeNull()
    expect(foldWslUncPathCaseInsensitiveParts('//server/share/x')).toBeNull()
    expect(foldWslUncPathCaseInsensitiveParts('/home/jin')).toBeNull()
  })
})

describe('mapPosixPathToWslWorktreeUncPath', () => {
  it('rebases a POSIX path onto the WSL worktree UNC share', () => {
    expect(
      mapPosixPathToWslWorktreeUncPath(
        '/home/jin/repo/src/app.ts',
        '\\\\wsl.localhost\\Ubuntu\\home\\jin\\repo'
      )
    ).toBe('\\\\wsl.localhost\\Ubuntu\\home\\jin\\repo\\src\\app.ts')
  })

  it('preserves the legacy wsl$ share spelling of the worktree', () => {
    expect(
      mapPosixPathToWslWorktreeUncPath('/etc/hosts', '\\\\wsl$\\Debian\\home\\jin')
    ).toBe('\\\\wsl$\\Debian\\etc\\hosts')
  })

  it('maps paths outside the worktree, including drvfs mounts', () => {
    expect(
      mapPosixPathToWslWorktreeUncPath('/mnt/c/logs/out.txt', '\\\\wsl.localhost\\Ubuntu\\repo')
    ).toBe('\\\\wsl.localhost\\Ubuntu\\mnt\\c\\logs\\out.txt')
  })

  it('returns null for non-POSIX paths and non-WSL worktrees', () => {
    const wslWorktree = '\\\\wsl.localhost\\Ubuntu\\repo'
    expect(mapPosixPathToWslWorktreeUncPath('C:\\Users\\jin\\a.ts', wslWorktree)).toBeNull()
    expect(mapPosixPathToWslWorktreeUncPath('\\\\wsl$\\Ubuntu\\repo\\a.ts', wslWorktree)).toBeNull()
    expect(mapPosixPathToWslWorktreeUncPath('//server/share/a.ts', wslWorktree)).toBeNull()
    expect(mapPosixPathToWslWorktreeUncPath('/home/jin/a.ts', 'C:\\repo')).toBeNull()
    expect(mapPosixPathToWslWorktreeUncPath('/home/jin/a.ts', '/home/jin/repo')).toBeNull()
  })
})
