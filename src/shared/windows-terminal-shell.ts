import type { AgentStartupShell } from './tui-agent-startup-shell'
import { WINDOWS_NUSHELL_SHELL, isNushellExecutableName } from './nushell-shell'

export const WINDOWS_GIT_BASH_SHELL = 'git-bash'

// Why: the nushell sentinel lives beside the classification vocabulary in nushell-shell.ts; re-export so Windows shell consumers keep one import site.
export { WINDOWS_NUSHELL_SHELL } from './nushell-shell'

export type BuiltInWindowsTerminalShell =
  | 'powershell.exe'
  | 'cmd.exe'
  | 'wsl.exe'
  | typeof WINDOWS_GIT_BASH_SHELL
  | typeof WINDOWS_NUSHELL_SHELL

/**
 * Classifies a configured `terminalWindowsShell` value into the startup-shell
 * family used to quote queued commands. Git Bash / wsl.exe run a POSIX shell;
 * cmd.exe needs cmd quoting; everything else (PowerShell, pwsh, unknown) is
 * treated as PowerShell, matching the Windows default.
 */
export function resolveWindowsShellStartupFamily(
  shell: string | null | undefined
): AgentStartupShell {
  const trimmed = shell?.trim()
  if (!trimmed) {
    return 'powershell'
  }
  if (trimmed === WINDOWS_GIT_BASH_SHELL) {
    return 'posix'
  }
  // Why: nu parses neither POSIX nor PowerShell quoting; queued commands need the nushell dialect.
  if (trimmed === WINDOWS_NUSHELL_SHELL || isNushellExecutableName(trimmed)) {
    return 'nushell'
  }
  const basename = trimmed.replaceAll('\\', '/').split('/').pop()?.toLowerCase() ?? ''
  if (basename === 'cmd.exe') {
    return 'cmd'
  }
  // Why: wsl.exe and bash.exe (Git for Windows) launch POSIX shells, so queued
  // commands must use POSIX quoting and `cd '<cwd>'` rather than cmd/PowerShell.
  if (basename === 'wsl.exe' || basename === 'wsl' || basename === 'bash.exe') {
    return 'posix'
  }
  return 'powershell'
}

export function resolveLocalWindowsAgentStartupShell(args: {
  platform: NodeJS.Platform
  isRemote: boolean
  terminalWindowsShell?: string | null
}): AgentStartupShell | undefined {
  // Why: terminalWindowsShell describes the local host shell; SSH/remote
  // targets need their own shell signal before we can safely override quoting.
  if (args.platform !== 'win32' || args.isRemote) {
    return undefined
  }
  return resolveWindowsShellStartupFamily(args.terminalWindowsShell)
}
