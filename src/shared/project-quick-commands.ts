import { supportsTerminalAgentQuickCommand } from './terminal-quick-commands'
import type { OrcaHooks, OrcaProjectQuickCommand, TerminalQuickCommand } from './types'

/**
 * Quick commands defined in a repo's committed `orca.yaml` ("project" quick
 * commands, #8481), as opposed to the user's local Settings quick commands.
 * They are repo-controlled input: every dispatch path must pass the shared
 * orca.yaml trust gate (see orca-yaml-trust-content.ts).
 */

// Why: settings ids are user-typed but always compared verbatim, so a local id
// spoofing this prefix only subjects itself to the stricter project trust gate.
export const PROJECT_QUICK_COMMAND_ID_PREFIX = 'orcaYaml:'

export function isProjectQuickCommand(command: Pick<TerminalQuickCommand, 'id'>): boolean {
  return command.id.startsWith(PROJECT_QUICK_COMMAND_ID_PREFIX)
}

export function isOrcaProjectAgentQuickCommand(
  command: OrcaProjectQuickCommand
): command is Extract<OrcaProjectQuickCommand, { action: 'agent-prompt' }> {
  return 'action' in command && command.action === 'agent-prompt'
}

/**
 * Convert parsed orca.yaml quick commands into dispatchable TerminalQuickCommands
 * scoped to the repo, with stable derived ids (`orcaYaml:<repoId>:<index>`).
 * Agent entries whose agent Orca cannot inject a prompt into are dropped.
 */
export function projectQuickCommandsForRepo(
  repoId: string,
  hooks: Pick<OrcaHooks, 'quickCommands'> | null
): TerminalQuickCommand[] {
  const converted = (hooks?.quickCommands ?? []).map((entry, index): TerminalQuickCommand | null => {
    const base = {
      id: `${PROJECT_QUICK_COMMAND_ID_PREFIX}${repoId}:${index}`,
      label: entry.label,
      scope: { type: 'repo', repoId } as const
    }
    if (isOrcaProjectAgentQuickCommand(entry)) {
      if (!supportsTerminalAgentQuickCommand(entry.agent)) {
        return null
      }
      return { ...base, action: 'agent-prompt', agent: entry.agent, prompt: entry.prompt }
    }
    return {
      ...base,
      action: 'terminal-command',
      command: entry.command,
      appendEnter: entry.appendEnter !== false
    }
  })
  return converted.filter((entry): entry is TerminalQuickCommand => entry !== null)
}

/**
 * Local override rule (#8481): a local Settings quick command with the same
 * label as a project one wins — the project twin is hidden, not merged.
 */
export function projectQuickCommandsNotOverriddenByLocal(
  localCommands: readonly TerminalQuickCommand[],
  projectCommands: readonly TerminalQuickCommand[]
): TerminalQuickCommand[] {
  const localLabels = new Set(localCommands.map((command) => command.label.trim()))
  return projectCommands.filter((command) => !localLabels.has(command.label.trim()))
}
