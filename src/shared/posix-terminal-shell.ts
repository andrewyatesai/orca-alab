/**
 * Shared vocabulary for the macOS/Linux default terminal shell setting
 * (`terminalPosixShell`, #5097) — the POSIX counterpart of
 * `windows-terminal-shell.ts`. The setting stores a shell name ('zsh') or an
 * explicit path; main resolves it to an executable before spawn and SSH
 * terminals always keep the remote login shell.
 */

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
