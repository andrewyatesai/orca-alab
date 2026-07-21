import {
  buildSetupRunnerCommand as buildSharedSetupRunnerCommand,
  getSetupRunnerCommandPlatformForPath
} from '../../../shared/setup-runner-command'
import type { AgentStartupShell } from '../../../shared/tui-agent-startup-shell'
import { resolveLocalWindowsAgentStartupShell } from '../../../shared/windows-terminal-shell'
import { repoIsRemote } from '../../../shared/agent-launch-remote'
import { LOCAL_EXECUTION_HOST_ID } from '../../../shared/execution-host'

export function buildSetupRunnerCommand(
  runnerScriptPath: string,
  terminalShellFamily?: AgentStartupShell
): string {
  // Why: the runner may live on a remote/WSL filesystem, so the shell follows
  // the runner path format rather than the local renderer OS.
  return buildSharedSetupRunnerCommand(
    runnerScriptPath,
    getSetupRunnerCommandPlatformForPath(
      runnerScriptPath,
      navigator.userAgent.includes('Windows') ? 'windows' : 'posix'
    ),
    terminalShellFamily
  )
}

// Why: setup commands are typed into the tab's interactive shell, so delivery
// must match the configured Windows shell — a cmd.exe wrapper breaks in Git Bash (#6896).
export function getWorktreeSetupTerminalShellFamily(
  state: {
    repos?: readonly { id: string; connectionId?: string | null }[]
    worktreesByRepo?: Record<string, readonly { id: string; repoId: string; hostId?: string }[]>
  },
  worktreeId: string,
  terminalWindowsShell: string | null | undefined
): AgentStartupShell | undefined {
  const worktree = Object.values(state.worktreesByRepo ?? {})
    .flat()
    .find((entry) => entry.id === worktreeId)
  const repo = worktree ? state.repos?.find((entry) => entry.id === worktree.repoId) : undefined
  const isRemote =
    (repo ? repoIsRemote(repo) : false) ||
    Boolean(worktree?.hostId && worktree.hostId !== LOCAL_EXECUTION_HOST_ID)
  return resolveLocalWindowsAgentStartupShell({
    platform: navigator.userAgent.includes('Windows') ? 'win32' : 'linux',
    isRemote,
    terminalWindowsShell
  })
}
