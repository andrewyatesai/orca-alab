// TS dispatch for the commit-message-agent-spec parity module: maps the shared
// vector function names to the real `src/shared/commit-message-agent-spec.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  getCommitMessageAgentCapability,
  getCommitMessageModel,
  getCommitMessageModelCapability,
  isCustomAgentId,
  listCommitMessageAgentCapabilities,
  listCommitMessageAgentIds,
  resolveCommitMessageAgentChoice,
  type CommitMessageAgentChoice,
  type DefaultTuiAgentPreference
} from '../../../src/shared/commit-message-agent-spec'
import type { TuiAgent } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isCustomAgentId':
      // Single raw arg: JSON null stands in for both TS null and undefined; both yield false.
      return isCustomAgentId(input as string | null | undefined)
    case 'resolveCommitMessageAgentChoice': {
      const { configuredAgentId, defaultTuiAgent, disabledTuiAgents } = input as {
        configuredAgentId?: CommitMessageAgentChoice | null
        defaultTuiAgent?: DefaultTuiAgentPreference
        disabledTuiAgents?: unknown[] | null
      }
      return resolveCommitMessageAgentChoice(configuredAgentId, defaultTuiAgent, disabledTuiAgents)
    }
    case 'getCommitMessageModel': {
      const { agentId, modelId } = input as { agentId: TuiAgent; modelId: string }
      return getCommitMessageModel(agentId, modelId)
    }
    case 'getCommitMessageAgentCapability': {
      const { agentId } = input as { agentId: TuiAgent }
      return getCommitMessageAgentCapability(agentId)
    }
    case 'getCommitMessageModelCapability': {
      const { agentId, modelId } = input as { agentId: TuiAgent; modelId: string }
      return getCommitMessageModelCapability(agentId, modelId)
    }
    case 'listCommitMessageAgentIds':
      return listCommitMessageAgentIds()
    case 'listCommitMessageAgentCapabilities':
      return listCommitMessageAgentCapabilities()
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
