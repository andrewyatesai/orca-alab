import { existsSync } from 'node:fs'
import { win32 as pathWin32 } from 'node:path'
import { WINDOWS_NUSHELL_SHELL, isNushellExecutableName } from '../shared/nushell-shell'

/** Windows nu.exe resolution (#8928 §3.3) — mirror of git-bash.ts. */

type WindowsNushellPathOptions = {
  env?: NodeJS.ProcessEnv
  exists?: (path: string) => boolean
  platform?: NodeJS.Platform
}

function readEnv(env: NodeJS.ProcessEnv, names: string[]): string | undefined {
  for (const name of names) {
    const value = env[name]
    if (value) {
      return value
    }
  }
  return undefined
}

function normalizePathSegment(segment: string): string {
  const trimmed = segment.trim()
  return trimmed.startsWith('"') && trimmed.endsWith('"') ? trimmed.slice(1, -1) : trimmed
}

function isWindowsAppsAliasDir(directory: string): boolean {
  return pathWin32.normalize(directory).toLowerCase().endsWith('\\microsoft\\windowsapps')
}

function pushCandidate(
  candidates: string[],
  seen: Set<string>,
  candidate: string | undefined
): void {
  if (!candidate) {
    return
  }
  const normalized = pathWin32.normalize(candidate)
  const key = normalized.toLowerCase()
  if (!seen.has(key)) {
    seen.add(key)
    candidates.push(normalized)
  }
}

export function getWindowsNushellCandidatePaths(env: NodeJS.ProcessEnv = process.env): string[] {
  const candidates: string[] = []
  const seen = new Set<string>()
  const programFiles = readEnv(env, ['ProgramFiles', 'PROGRAMFILES'])
  const localAppData = readEnv(env, ['LOCALAPPDATA', 'LocalAppData'])
  const userProfile = readEnv(env, ['USERPROFILE', 'UserProfile'])
  const programData = readEnv(env, ['ProgramData', 'PROGRAMDATA'])

  if (programFiles) {
    pushCandidate(candidates, seen, pathWin32.join(programFiles, 'nu', 'bin', 'nu.exe'))
  }
  if (localAppData) {
    pushCandidate(candidates, seen, pathWin32.join(localAppData, 'Programs', 'nu', 'bin', 'nu.exe'))
  }
  if (userProfile) {
    pushCandidate(candidates, seen, pathWin32.join(userProfile, 'scoop', 'shims', 'nu.exe'))
  }
  if (programData) {
    pushCandidate(candidates, seen, pathWin32.join(programData, 'chocolatey', 'bin', 'nu.exe'))
  }
  if (userProfile) {
    pushCandidate(candidates, seen, pathWin32.join(userProfile, '.cargo', 'bin', 'nu.exe'))
  }

  const pathValue = readEnv(env, ['Path', 'PATH'])
  if (pathValue) {
    for (const rawSegment of pathValue.split(pathWin32.delimiter)) {
      const segment = normalizePathSegment(rawSegment)
      // Why: the WindowsApps segment holds the Store execution alias, which must stay last (CreateProcessW-stub risk).
      if (!segment || isWindowsAppsAliasDir(segment)) {
        continue
      }
      pushCandidate(candidates, seen, pathWin32.join(segment, 'nu.exe'))
    }
  }

  if (localAppData) {
    pushCandidate(
      candidates,
      seen,
      pathWin32.join(localAppData, 'Microsoft', 'WindowsApps', 'nu.exe')
    )
  }

  return candidates
}

export function resolveWindowsNushellPath(options: WindowsNushellPathOptions = {}): string | null {
  const platform = options.platform ?? process.platform
  if (platform !== 'win32') {
    return null
  }
  const exists = options.exists ?? existsSync
  for (const candidate of getWindowsNushellCandidatePaths(options.env ?? process.env)) {
    if (exists(candidate)) {
      return candidate
    }
  }
  return null
}

export function isNushellAvailable(): boolean {
  return resolveWindowsNushellPath() !== null
}

/** Sentinel or explicit nu path -> absolute nu.exe; null when not a nushell pick or not installed. */
export function resolveWindowsNushellShellPath(
  shell: string,
  options: WindowsNushellPathOptions = {}
): string | null {
  const trimmed = shell.trim()
  if (!trimmed) {
    return null
  }
  if (trimmed === WINDOWS_NUSHELL_SHELL) {
    return resolveWindowsNushellPath(options)
  }
  if (!isNushellExecutableName(trimmed)) {
    return null
  }
  // Why: an explicit path is honored as-is (same contract as git-bash); only bare names re-resolve.
  if (pathWin32.isAbsolute(trimmed) || trimmed.includes('\\') || trimmed.includes('/')) {
    return trimmed
  }
  return resolveWindowsNushellPath(options)
}
