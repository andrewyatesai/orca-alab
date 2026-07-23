import { win32 as pathWin32 } from 'node:path'

export type WindowsPowerShellImplementation = 'auto' | 'powershell.exe' | 'pwsh.exe'
export type WindowsPowerShellShellFamily =
  | 'powershell.exe'
  | 'pwsh.exe'
  | 'cmd.exe'
  | 'wsl.exe'
  | undefined

/** Whether the win32 spawn path may re-resolve the configured shell through the
 *  PowerShell family resolver.
 *
 *  Why (#7467): an explicit absolute custom path must spawn verbatim — family
 *  re-resolution would clobber it with the discovered install whenever a
 *  PowerShell implementation preference is set. Bare names keep today's rules.
 */
export function shouldResolveWindowsPowerShellFamily(args: {
  shellSetting: string
  implementation: WindowsPowerShellImplementation | undefined
}): boolean {
  if (pathWin32.isAbsolute(args.shellSetting)) {
    return false
  }
  return (
    args.implementation !== undefined ||
    pathWin32.basename(args.shellSetting) === args.shellSetting
  )
}

export function shouldProbeWindowsPowerShellAvailability(args: {
  shellFamily: WindowsPowerShellShellFamily
  implementation: WindowsPowerShellImplementation | undefined
}): boolean {
  return (
    args.shellFamily === 'powershell.exe' &&
    (args.implementation === undefined || args.implementation === 'auto')
  )
}

/** Resolve which PowerShell executable to spawn right now on Windows.
 *
 * Why: explicit pwsh.exe choices must not be downgraded by a transient cold
 * availability probe; the spawn chain handles true absence with a safe fallback.
 */
export function resolveEffectiveWindowsPowerShell(args: {
  shellFamily: WindowsPowerShellShellFamily
  implementation: WindowsPowerShellImplementation | undefined
  pwshAvailable: boolean
}): 'powershell.exe' | 'pwsh.exe' | null {
  if (args.shellFamily === 'pwsh.exe') {
    return 'pwsh.exe'
  }

  if (args.shellFamily !== 'powershell.exe') {
    return null
  }

  if (args.implementation === 'powershell.exe') {
    return 'powershell.exe'
  }

  if (args.implementation === 'pwsh.exe') {
    return 'pwsh.exe'
  }

  if (args.pwshAvailable) {
    return 'pwsh.exe'
  }

  return 'powershell.exe'
}
