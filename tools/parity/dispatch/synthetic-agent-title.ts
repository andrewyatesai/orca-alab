// TS dispatch for the synthetic-agent-title parity module: maps the shared
// vector function names to the real `src/shared/synthetic-agent-title.ts`
// exports so the harness compares the live TS reference against the Rust port.

import type { AgentStatusState, AgentType } from '../../../src/shared/agent-status-types'
import {
  getSyntheticAgentTerminalTitle,
  getSyntheticAgentTitleProfile,
  shouldDriveSyntheticAgentTitleFromHook
} from '../../../src/shared/synthetic-agent-title'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'getSyntheticAgentTitleProfile':
      return getSyntheticAgentTitleProfile(input as AgentType | null)
    case 'getSyntheticAgentTerminalTitle': {
      const { agentType, state } = input as { agentType: AgentType | null; state: AgentStatusState }
      return getSyntheticAgentTerminalTitle(agentType, state)
    }
    case 'shouldDriveSyntheticAgentTitleFromHook': {
      const { agentType, state } = input as { agentType: AgentType | null; state: AgentStatusState }
      return shouldDriveSyntheticAgentTitleFromHook(agentType, state)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
