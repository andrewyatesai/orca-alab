import { formatAgentTypeLabel } from './agent-type-label'
import type { AgentType } from './agent-status-types'

export const COMMAND_NOT_FOUND_EXIT_CODE = 127
export const COMMAND_NOT_EXECUTABLE_EXIT_CODE = 126
// Why: 127/126 prove the launch command itself never ran, but only near the
// launch seed — a later shell command can also return 127 (#7047).
export const AGENT_LAUNCH_FAILURE_SEED_WINDOW_MS = 60_000
// Why: other non-zero exits are ambiguous; only an immediate death after the
// seed reads as "failed to launch" rather than a finished agent run.
export const AGENT_LAUNCH_FAILURE_IMMEDIATE_EXIT_WINDOW_MS = 10_000

/** Classify a pre-recognition command exit as an agent launch failure (#7047).
 *  Callers must already know no agent recognition evidence (hook update, agent
 *  title, recognized process) arrived since the status row was seeded. */
export function isAgentLaunchFailureExit(args: {
  exitCode: number | null
  /** Elapsed ms since the launch seeded the pane's working status row. */
  msSinceStatusSeed: number
}): boolean {
  const { exitCode, msSinceStatusSeed } = args
  if (exitCode === null || exitCode === 0 || msSinceStatusSeed < 0) {
    return false
  }
  if (exitCode === COMMAND_NOT_FOUND_EXIT_CODE || exitCode === COMMAND_NOT_EXECUTABLE_EXIT_CODE) {
    return msSinceStatusSeed <= AGENT_LAUNCH_FAILURE_SEED_WINDOW_MS
  }
  return msSinceStatusSeed <= AGENT_LAUNCH_FAILURE_IMMEDIATE_EXIT_WINDOW_MS
}

/** Human-readable failure detail for the status row's message line.
 *  Says "this host" because the CLI may be missing on an SSH/remote host
 *  rather than the local machine. */
export function agentLaunchFailureMessage(
  exitCode: number,
  agentType: AgentType | null | undefined
): string {
  const label = formatAgentTypeLabel(agentType)
  if (exitCode === COMMAND_NOT_FOUND_EXIT_CODE) {
    return `${label} failed to launch: command not found (exit 127). Install the CLI on this host and try again.`
  }
  if (exitCode === COMMAND_NOT_EXECUTABLE_EXIT_CODE) {
    return `${label} failed to launch: command not executable (exit 126). Check the CLI installation on this host.`
  }
  return `${label} failed to launch (exit ${exitCode}).`
}
