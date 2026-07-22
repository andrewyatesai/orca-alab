import type { AgentStartupShell } from './tui-agent-startup-shell'
import { resolveLocalWindowsAgentStartupShell } from './windows-terminal-shell'
import { resolveLocalPosixAgentStartupShell } from './posix-terminal-shell'

/**
 * Quoting dialect for commands queued into a LOCAL terminal: the Windows shell
 * family on win32 launches, the POSIX/nu family on client-local POSIX launches,
 * undefined for remotes and cross-host (WSL-runtime) launches.
 */
export function resolveLocalAgentStartupShell(args: {
  /** Platform the launch targets (may differ from the client for WSL runtimes). */
  platform: NodeJS.Platform
  /** Platform of the host that owns the shell settings (CLIENT_PLATFORM / process.platform). */
  clientPlatform: NodeJS.Platform
  isRemote: boolean
  terminalWindowsShell?: string | null
  terminalPosixShell?: string | null
}): AgentStartupShell | undefined {
  return (
    resolveLocalWindowsAgentStartupShell({
      platform: args.platform,
      isRemote: args.isRemote,
      terminalWindowsShell: args.terminalWindowsShell
    }) ??
    resolveLocalPosixAgentStartupShell({
      platform: args.platform,
      clientPlatform: args.clientPlatform,
      isRemote: args.isRemote,
      terminalPosixShell: args.terminalPosixShell
    })
  )
}
