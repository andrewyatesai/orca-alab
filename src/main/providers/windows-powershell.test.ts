import { describe, expect, it } from 'vitest'
import {
  resolveEffectiveWindowsPowerShell,
  shouldResolveWindowsPowerShellFamily
} from './windows-powershell'

describe('shouldResolveWindowsPowerShellFamily', () => {
  it('never re-resolves an absolute custom path, even with an implementation preference (#7467)', () => {
    expect(
      shouldResolveWindowsPowerShellFamily({
        shellSetting: 'D:\\tools\\pwsh-daily\\pwsh.exe',
        implementation: 'powershell.exe'
      })
    ).toBe(false)
    expect(
      shouldResolveWindowsPowerShellFamily({
        shellSetting: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe',
        implementation: 'auto'
      })
    ).toBe(false)
  })

  it('keeps bare-name resolution rules unchanged', () => {
    expect(
      shouldResolveWindowsPowerShellFamily({ shellSetting: 'pwsh.exe', implementation: undefined })
    ).toBe(true)
    expect(
      shouldResolveWindowsPowerShellFamily({
        shellSetting: 'powershell.exe',
        implementation: 'pwsh.exe'
      })
    ).toBe(true)
    // Relative non-bare paths resolve only under an explicit implementation preference (pre-#7467 behavior).
    expect(
      shouldResolveWindowsPowerShellFamily({
        shellSetting: '.\\tools\\pwsh.exe',
        implementation: undefined
      })
    ).toBe(false)
    expect(
      shouldResolveWindowsPowerShellFamily({
        shellSetting: '.\\tools\\pwsh.exe',
        implementation: 'auto'
      })
    ).toBe(true)
  })
})

describe('resolveEffectiveWindowsPowerShell', () => {
  it('returns null for non-PowerShell shell families', () => {
    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'cmd.exe',
        implementation: 'pwsh.exe',
        pwshAvailable: true
      })
    ).toBeNull()

    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'wsl.exe',
        implementation: 'powershell.exe',
        pwshAvailable: true
      })
    ).toBeNull()

    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: undefined,
        implementation: 'pwsh.exe',
        pwshAvailable: true
      })
    ).toBeNull()
  })

  it('honors a direct pwsh.exe shell request even when the availability probe is false', () => {
    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'pwsh.exe',
        implementation: 'powershell.exe',
        pwshAvailable: false
      })
    ).toBe('pwsh.exe')
  })

  it('returns powershell.exe when the saved implementation is powershell.exe', () => {
    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'powershell.exe',
        implementation: 'powershell.exe',
        pwshAvailable: true
      })
    ).toBe('powershell.exe')
  })

  it('returns pwsh.exe when the saved implementation is pwsh.exe and pwsh is available', () => {
    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'powershell.exe',
        implementation: 'pwsh.exe',
        pwshAvailable: true
      })
    ).toBe('pwsh.exe')
  })

  it('keeps an explicit pwsh.exe preference when the availability probe is false', () => {
    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'powershell.exe',
        implementation: 'pwsh.exe',
        pwshAvailable: false
      })
    ).toBe('pwsh.exe')
  })

  it('uses pwsh.exe for Auto when pwsh is available', () => {
    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'powershell.exe',
        implementation: 'auto',
        pwshAvailable: true
      })
    ).toBe('pwsh.exe')
  })

  it('uses powershell.exe for Auto when pwsh is unavailable', () => {
    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'powershell.exe',
        implementation: 'auto',
        pwshAvailable: false
      })
    ).toBe('powershell.exe')
  })

  it('defaults to Auto when no implementation is persisted', () => {
    expect(
      resolveEffectiveWindowsPowerShell({
        shellFamily: 'powershell.exe',
        implementation: undefined,
        pwshAvailable: true
      })
    ).toBe('pwsh.exe')
  })
})
