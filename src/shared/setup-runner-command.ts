import { isWindowsAbsolutePathLike } from './cross-platform-path'
import type { AgentStartupShell } from './tui-agent-startup-shell'

export type SetupRunnerCommandPlatform = 'windows' | 'posix'
export type SetupRunnerCommandShell = 'posix' | 'windows'

export type SetupRunnerCommandResolution = {
  command: string
  runnerScriptPathForShell: string
  shell: SetupRunnerCommandShell
}

export function buildSetupRunnerCommand(
  runnerScriptPath: string,
  platform: SetupRunnerCommandPlatform,
  terminalShellFamily?: AgentStartupShell
): string {
  return resolveSetupRunnerCommand(runnerScriptPath, platform, terminalShellFamily).command
}

export function getSetupRunnerCommandPlatformForPath(
  runnerScriptPath: string,
  fallbackPlatform: SetupRunnerCommandPlatform
): SetupRunnerCommandPlatform {
  if (isWindowsAbsolutePathLike(runnerScriptPath)) {
    return 'windows'
  }
  if (runnerScriptPath.startsWith('/')) {
    return 'posix'
  }
  return fallbackPlatform
}

export function resolveSetupRunnerCommand(
  runnerScriptPath: string,
  platform: SetupRunnerCommandPlatform,
  terminalShellFamily?: AgentStartupShell
): SetupRunnerCommandResolution {
  if (platform === 'windows') {
    if (isWslUncPath(runnerScriptPath)) {
      const linuxPath = wslUncToLinuxPath(runnerScriptPath)
      return {
        command: `bash ${quotePosixArg(linuxPath)}`,
        runnerScriptPathForShell: linuxPath,
        shell: 'posix'
      }
    }
    if (runnerScriptPath.startsWith('/') && !isWindowsAbsolutePathLike(runnerScriptPath)) {
      return {
        command: `bash ${quotePosixArg(runnerScriptPath)}`,
        runnerScriptPathForShell: runnerScriptPath,
        shell: 'posix'
      }
    }
    if (terminalShellFamily === 'posix') {
      // Why: Git Bash history-expands `!` inside double quotes and MSYS-converts /c to C:\ (#6896);
      // single-quote the .cmd path, disable path conversion, and keep sequencing in POSIX form.
      return {
        command: `MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' cmd.exe /d /c ${quotePosixArg(runnerScriptPath)}`,
        runnerScriptPathForShell: runnerScriptPath.replace(/\\/g, '/'),
        shell: 'posix'
      }
    }
    return {
      command: `cmd.exe /c ${quoteWindowsArg(runnerScriptPath)}`,
      runnerScriptPathForShell: runnerScriptPath,
      shell: 'windows'
    }
  }

  return {
    command: `bash ${quotePosixArg(runnerScriptPath)}`,
    runnerScriptPathForShell: runnerScriptPath,
    shell: 'posix'
  }
}

export function isWslUncPath(path: string): boolean {
  const normalized = path.replace(/\\/g, '/')
  return /^\/\/(wsl\.localhost|wsl\$)\//i.test(normalized)
}

export function wslUncToLinuxPath(windowsPath: string): string {
  const normalized = windowsPath.replace(/\\/g, '/')
  const match = normalized.match(/^\/\/(wsl\.localhost|wsl\$)\/[^/]+(\/.*)?$/i)
  return match?.[2] || '/'
}

function quotePosixArg(value: string): string {
  if (/^[A-Za-z0-9_./:-]+$/.test(value)) {
    return value
  }

  return `'${value.replace(/'/g, `'\\''`)}'`
}

function quoteWindowsArg(value: string): string {
  return `"${value.replace(/"/g, '""')}"`
}
