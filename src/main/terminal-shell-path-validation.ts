import { accessSync, constants as fsConstants, statSync } from 'node:fs'
import { posix as pathPosix, win32 as pathWin32 } from 'node:path'
import {
  isWindowsAppExecutionAliasPath,
  resolveWindowsAppExecutionAliasTarget
} from './providers/windows-powershell-executable'
import type { ShellPathValidation } from '../shared/terminal-shell-path-validation'

/** Extensions ConPTY can launch directly as a shell executable. */
const WINDOWS_SHELL_EXECUTABLE_EXTENSIONS = new Set(['.exe', '.cmd', '.bat', '.com'])

type ShellPathStat = { isFile: boolean; isDirectory: boolean }

/** Dependency seams so the validator is unit-testable on any host OS. */
export type ShellPathValidationOptions = {
  platform?: NodeJS.Platform
  /** stat() the candidate; null when the path does not exist. */
  statPath?: (path: string) => ShellPathStat | null
  /** POSIX X_OK probe (ignored on win32, where extension + file-ness decide). */
  isExecutable?: (path: string) => boolean
  /** Resolves a Store App Execution Alias to its package executable. */
  resolveAppExecutionAlias?: (path: string) => string | null
}

function defaultStatPath(path: string): ShellPathStat | null {
  try {
    const stat = statSync(path)
    return { isFile: stat.isFile(), isDirectory: stat.isDirectory() }
  } catch {
    return null
  }
}

function defaultIsExecutable(path: string): boolean {
  try {
    accessSync(path, fsConstants.X_OK)
    return true
  } catch {
    return false
  }
}

function validateWindowsShellPath(
  rawPath: string,
  options: ShellPathValidationOptions
): ShellPathValidation {
  if (!pathWin32.isAbsolute(rawPath)) {
    return { ok: false, reason: 'not-absolute' }
  }
  const normalized = pathWin32.normalize(rawPath)
  // Why: a Store App Execution Alias stub is a reparse point ConPTY cannot
  // launch (ERROR_ACCESS_DENIED) — report it before stat, which sees a "file".
  if (isWindowsAppExecutionAliasPath(normalized)) {
    const resolveAlias = options.resolveAppExecutionAlias ?? resolveWindowsAppExecutionAliasTarget
    const target = resolveAlias(normalized)
    return {
      ok: false,
      reason: 'not-executable',
      ...(target && pathWin32.isAbsolute(target) && !isWindowsAppExecutionAliasPath(target)
        ? { resolvedPath: pathWin32.normalize(target) }
        : {})
    }
  }
  const stat = (options.statPath ?? defaultStatPath)(normalized)
  if (!stat) {
    return { ok: false, reason: 'not-found' }
  }
  if (stat.isDirectory) {
    return { ok: false, reason: 'is-directory' }
  }
  if (!stat.isFile) {
    return { ok: false, reason: 'not-executable' }
  }
  const extension = pathWin32.extname(normalized).toLowerCase()
  if (!WINDOWS_SHELL_EXECUTABLE_EXTENSIONS.has(extension)) {
    return { ok: false, reason: 'not-executable' }
  }
  return { ok: true, resolvedPath: normalized }
}

function validatePosixShellPath(
  rawPath: string,
  options: ShellPathValidationOptions
): ShellPathValidation {
  if (!pathPosix.isAbsolute(rawPath)) {
    return { ok: false, reason: 'not-absolute' }
  }
  const normalized = pathPosix.normalize(rawPath)
  const stat = (options.statPath ?? defaultStatPath)(normalized)
  if (!stat) {
    return { ok: false, reason: 'not-found' }
  }
  if (stat.isDirectory) {
    return { ok: false, reason: 'is-directory' }
  }
  if (!(options.isExecutable ?? defaultIsExecutable)(normalized)) {
    return { ok: false, reason: 'not-executable' }
  }
  return { ok: true, resolvedPath: normalized }
}

/**
 * Validate an explicit custom shell path (#7467) for the local terminal host.
 * SSH terminals keep the remote login shell, so this never probes remote hosts.
 */
export function validateTerminalShellPath(
  rawPath: string,
  options: ShellPathValidationOptions = {}
): ShellPathValidation {
  const platform = options.platform ?? process.platform
  const trimmed = rawPath.trim()
  if (!trimmed) {
    return { ok: false, reason: 'not-absolute' }
  }
  return platform === 'win32'
    ? validateWindowsShellPath(trimmed, options)
    : validatePosixShellPath(trimmed, options)
}
