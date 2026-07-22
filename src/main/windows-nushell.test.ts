import { describe, expect, it } from 'vitest'
import { win32 as pathWin32 } from 'node:path'
import {
  getWindowsNushellCandidatePaths,
  resolveWindowsNushellPath,
  resolveWindowsNushellShellPath
} from './windows-nushell'

const ENV: NodeJS.ProcessEnv = {
  ProgramFiles: 'C:\\Program Files',
  LOCALAPPDATA: 'C:\\Users\\alice\\AppData\\Local',
  USERPROFILE: 'C:\\Users\\alice',
  ProgramData: 'C:\\ProgramData',
  Path: 'C:\\Users\\alice\\AppData\\Local\\Microsoft\\WindowsApps;C:\\custom\\nu-dir'
}

const WINDOWS_APPS_ALIAS = pathWin32.normalize(
  'C:\\Users\\alice\\AppData\\Local\\Microsoft\\WindowsApps\\nu.exe'
)

describe('getWindowsNushellCandidatePaths', () => {
  it('prefers winget/scoop/choco/cargo installs over the WindowsApps alias', () => {
    const candidates = getWindowsNushellCandidatePaths(ENV)
    expect(candidates).toEqual([
      'C:\\Program Files\\nu\\bin\\nu.exe',
      'C:\\Users\\alice\\AppData\\Local\\Programs\\nu\\bin\\nu.exe',
      'C:\\Users\\alice\\scoop\\shims\\nu.exe',
      'C:\\ProgramData\\chocolatey\\bin\\nu.exe',
      'C:\\Users\\alice\\.cargo\\bin\\nu.exe',
      'C:\\custom\\nu-dir\\nu.exe',
      WINDOWS_APPS_ALIAS
    ])
    // Why: the Store execution alias is a CreateProcessW stub risk and must stay last.
    expect(candidates.at(-1)).toBe(WINDOWS_APPS_ALIAS)
  })

  it('excludes the WindowsApps PATH segment from the PATH scan', () => {
    const candidates = getWindowsNushellCandidatePaths(ENV)
    expect(candidates.filter((candidate) => candidate === WINDOWS_APPS_ALIAS)).toHaveLength(1)
  })
})

describe('resolveWindowsNushellPath', () => {
  it('returns the first existing candidate', () => {
    const cargoNu = 'C:\\Users\\alice\\.cargo\\bin\\nu.exe'
    const resolved = resolveWindowsNushellPath({
      env: ENV,
      platform: 'win32',
      exists: (path) => path === cargoNu || path === WINDOWS_APPS_ALIAS
    })
    expect(resolved).toBe(cargoNu)
  })

  it('falls back to the WindowsApps alias only when nothing else exists', () => {
    const resolved = resolveWindowsNushellPath({
      env: ENV,
      platform: 'win32',
      exists: (path) => path === WINDOWS_APPS_ALIAS
    })
    expect(resolved).toBe(WINDOWS_APPS_ALIAS)
  })

  it('returns null off Windows and when nu is not installed', () => {
    expect(resolveWindowsNushellPath({ env: ENV, platform: 'darwin', exists: () => true })).toBe(
      null
    )
    expect(resolveWindowsNushellPath({ env: ENV, platform: 'win32', exists: () => false })).toBe(
      null
    )
  })
})

describe('resolveWindowsNushellShellPath', () => {
  it('resolves the nushell sentinel to an installed nu.exe', () => {
    const scoopNu = 'C:\\Users\\alice\\scoop\\shims\\nu.exe'
    const resolved = resolveWindowsNushellShellPath('nushell', {
      env: ENV,
      platform: 'win32',
      exists: (path) => path === scoopNu
    })
    expect(resolved).toBe(scoopNu)
  })

  it('honors an explicit nu.exe path as-is', () => {
    expect(
      resolveWindowsNushellShellPath('D:\\tools\\nu\\nu.exe', {
        env: ENV,
        platform: 'win32',
        exists: () => false
      })
    ).toBe('D:\\tools\\nu\\nu.exe')
  })

  it('re-resolves a bare nu.exe name through the candidate chain', () => {
    const cargoNu = 'C:\\Users\\alice\\.cargo\\bin\\nu.exe'
    expect(
      resolveWindowsNushellShellPath('nu.exe', {
        env: ENV,
        platform: 'win32',
        exists: (path) => path === cargoNu
      })
    ).toBe(cargoNu)
  })

  it('returns null for non-nushell shells and a missing install', () => {
    expect(resolveWindowsNushellShellPath('powershell.exe', { env: ENV, platform: 'win32' })).toBe(
      null
    )
    expect(
      resolveWindowsNushellShellPath('nushell', {
        env: ENV,
        platform: 'win32',
        exists: () => false
      })
    ).toBe(null)
  })
})
