import type { OrcaHooks } from './types'
import { isOrcaProjectAgentQuickCommand } from './project-quick-commands'

/**
 * Canonical text whose hash gates every command a committed `orca.yaml` can
 * inject into a shell: the setup script, defaultTabs commands, and project
 * quickCommands. Main, runtime, and renderer must hash the SAME text or trust
 * approvals stop matching across surfaces — extend it here only.
 */
export function getSharedCommandTrustContent(hooks: OrcaHooks | null): string {
  const tabCommands = (hooks?.defaultTabs ?? [])
    .map((tab, index) => {
      const command = tab.command?.trim()
      if (!command) {
        return null
      }
      const label = tab.title ? ` ${tab.title}` : ''
      return `# defaultTabs[${index + 1}]${label}\n${command}`
    })
    .filter((entry): entry is string => entry !== null)
  // Why: the agent id is part of the trusted bytes — swapping `claude` for a
  // malicious CLI while keeping the prompt identical must re-prompt.
  const quickCommands = (hooks?.quickCommands ?? []).map((command, index) => {
    const body = isOrcaProjectAgentQuickCommand(command)
      ? `agent-prompt ${command.agent}: ${command.prompt}`
      : command.command
    return `# quickCommands[${index + 1}] ${command.label}\n${body}`
  })
  return [hooks?.scripts?.setup?.trim(), ...tabCommands, ...quickCommands]
    .filter(Boolean)
    .join('\n\n')
}
