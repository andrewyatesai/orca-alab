/**
 * Shared vocabulary for the macOS/Linux default terminal shell setting
 * (`terminalPosixShell`, #5097) — the POSIX counterpart of
 * `windows-terminal-shell.ts`. The setting stores a shell name ('zsh') or an
 * explicit path; main resolves it to an executable before spawn and SSH
 * terminals always keep the remote login shell.
 */
import type { AgentStartupShell } from './tui-agent-startup-shell'
import { isNushellExecutableName } from './nushell-shell'

export const POSIX_TERMINAL_SHELL_CHOICES = ['zsh', 'bash', 'fish', 'nu'] as const
export type PosixTerminalShellChoice = (typeof POSIX_TERMINAL_SHELL_CHOICES)[number]

export type PosixTerminalShellOption = {
  shell: PosixTerminalShellChoice
  path: string
}

export type PosixTerminalShellDetection = {
  /** Installed selectable shells with their resolved executable paths. */
  shells: PosixTerminalShellOption[]
  /** Basename of the host's $SHELL, shown as the "System" option caption. */
  systemShellName: string | null
}

/** Empty/whitespace settings mean "system default" ($SHELL). */
export function normalizePosixShellSetting(value: string | null | undefined): string | null {
  const trimmed = value?.trim()
  return trimmed ? trimmed : null
}

/** Display name for both bare names and paths ('/usr/local/bin/fish' → 'fish'). */
export function posixShellDisplayName(value: string): string {
  return value.split('/').pop() || value
}

/**
 * POSIX mirror of `resolveLocalWindowsAgentStartupShell` (#8928 §5): picks the
 * quoting dialect for commands queued into local macOS/Linux terminals.
 */
export function resolveLocalPosixAgentStartupShell(args: {
  /** Platform the launch actually targets (may differ from the client for WSL runtimes). */
  platform: NodeJS.Platform
  /** Platform of the host that owns `terminalPosixShell`; pass CLIENT_PLATFORM / process.platform. */
  clientPlatform?: NodeJS.Platform
  isRemote: boolean
  terminalPosixShell?: string | null
}): AgentStartupShell | undefined {
  // Why: terminalPosixShell describes the local host default; SSH remotes keep 'posix' — the remote shell kind is unknown (documented limitation).
  if (args.platform === 'win32' || args.isRemote) {
    return undefined
  }
  // Why: a WSL-runtime launch (win32 client, linux target) enters the distro login shell, which terminalPosixShell does not describe.
  if (args.clientPlatform !== undefined && args.clientPlatform !== args.platform) {
    return undefined
  }
  const setting = normalizePosixShellSetting(args.terminalPosixShell)
  return setting && isNushellExecutableName(setting) ? 'nushell' : 'posix'
}
