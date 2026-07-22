import { accessSync, constants as fsConstants, readFileSync } from 'node:fs'
import { delimiter, posix as pathPosix } from 'node:path'
import {
  normalizePosixShellSetting,
  POSIX_TERMINAL_SHELL_CHOICES,
  type PosixTerminalShellDetection,
  type PosixTerminalShellOption
} from '../shared/posix-terminal-shell'

export type PosixShellProbeOptions = {
  platform?: NodeJS.Platform
  env?: NodeJS.ProcessEnv
  isExecutable?: (path: string) => boolean
  readEtcShells?: () => string
}

// Why: /etc/shells misses Homebrew/Nix installs and PATH misses login-shell-only entries; the static dirs backstop a minimal env.
const POSIX_SHELL_STATIC_DIRS = ['/bin', '/usr/bin', '/usr/local/bin', '/opt/homebrew/bin']
const HOME_RELATIVE_SHELL_DIRS = ['.cargo/bin', '.local/bin']

function defaultIsExecutable(path: string): boolean {
  try {
    accessSync(path, fsConstants.X_OK)
    return true
  } catch {
    return false
  }
}

function defaultReadEtcShells(): string {
  try {
    return readFileSync('/etc/shells', 'utf8')
  } catch {
    return ''
  }
}

function getPosixShellCandidatePaths(shellName: string, options: PosixShellProbeOptions): string[] {
  const candidates: string[] = []
  const seen = new Set<string>()
  const push = (candidate: string): void => {
    if (!seen.has(candidate)) {
      seen.add(candidate)
      candidates.push(candidate)
    }
  }
  const readEtcShells = options.readEtcShells ?? defaultReadEtcShells
  for (const line of readEtcShells().split('\n')) {
    const entry = line.trim()
    if (!entry || entry.startsWith('#')) {
      continue
    }
    if (pathPosix.basename(entry) === shellName) {
      push(entry)
    }
  }
  const env = options.env ?? process.env
  for (const dir of (env.PATH ?? '').split(delimiter)) {
    // Why: only rooted PATH entries — a relative segment would resolve against an arbitrary cwd.
    if (dir.startsWith('/')) {
      push(pathPosix.join(dir, shellName))
    }
  }
  // Why: cargo/user-local installs (nu especially) are often absent from both /etc/shells and a GUI-launched PATH.
  const home = env.HOME
  if (home?.startsWith('/')) {
    for (const dir of HOME_RELATIVE_SHELL_DIRS) {
      push(pathPosix.join(home, dir, shellName))
    }
  }
  for (const dir of POSIX_SHELL_STATIC_DIRS) {
    push(pathPosix.join(dir, shellName))
  }
  return candidates
}

/**
 * Resolves the `terminalPosixShell` setting to an executable path, or null
 * when unset/unresolvable (callers then keep the $SHELL default). Accepts a
 * bare shell name ('fish') or an explicit path ('/usr/local/bin/fish').
 */
export function resolvePosixShellSettingPath(
  setting: string | null | undefined,
  options: PosixShellProbeOptions = {}
): string | null {
  const platform = options.platform ?? process.platform
  if (platform === 'win32') {
    return null
  }
  const normalized = normalizePosixShellSetting(setting)
  if (!normalized) {
    return null
  }
  const isExecutable = options.isExecutable ?? defaultIsExecutable
  if (normalized.includes('/')) {
    return isExecutable(normalized) ? normalized : null
  }
  for (const candidate of getPosixShellCandidatePaths(normalized, options)) {
    if (isExecutable(candidate)) {
      return candidate
    }
  }
  return null
}

/** Availability probe behind `posixShells:detect` / `host.posixShells.detect`. */
export function detectPosixTerminalShells(
  options: PosixShellProbeOptions = {}
): PosixTerminalShellDetection {
  const platform = options.platform ?? process.platform
  if (platform === 'win32') {
    return { shells: [], systemShellName: null }
  }
  const shells: PosixTerminalShellOption[] = []
  for (const shell of POSIX_TERMINAL_SHELL_CHOICES) {
    const path = resolvePosixShellSettingPath(shell, options)
    if (path) {
      shells.push({ shell, path })
    }
  }
  const systemShell = (options.env ?? process.env).SHELL?.trim()
  return { shells, systemShellName: systemShell ? pathPosix.basename(systemShell) : null }
}

/**
 * Spawn-time fold mirroring `resolveLocalWindowsTerminalRuntimeOptions`: an
 * explicit per-tab override wins, else the global setting resolves to a path.
 * Callers gate on local non-Windows spawns — SSH keeps the remote login shell.
 */
export function resolveLocalPosixShellOverride(
  requestedShellOverride: string | undefined,
  terminalPosixShell: string | null | undefined,
  options: PosixShellProbeOptions = {}
): string | undefined {
  if (requestedShellOverride) {
    return requestedShellOverride
  }
  return resolvePosixShellSettingPath(terminalPosixShell, options) ?? undefined
}
