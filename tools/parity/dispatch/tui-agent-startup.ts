// TS dispatch for the tui-agent-startup parity module: maps the shared vector
// function names to the real `src/shared/tui-agent-startup.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  buildAgentDraftLaunchPlan,
  buildAgentStartupPlan,
  type AgentStartupShell
} from '../../../src/shared/tui-agent-startup'
import type { TuiAgent } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildAgentStartupPlan':
      return (
        buildAgentStartupPlan(
          input as {
            agent: TuiAgent
            prompt: string
            cmdOverrides: Partial<Record<TuiAgent, string>>
            platform: NodeJS.Platform
            shell?: AgentStartupShell
            allowEmptyPromptLaunch?: boolean
          }
        ) ?? null
      )
    case 'buildAgentDraftLaunchPlan':
      return (
        buildAgentDraftLaunchPlan(
          input as {
            agent: TuiAgent
            draft: string
            cmdOverrides: Partial<Record<TuiAgent, string>>
            platform: NodeJS.Platform
            shell?: AgentStartupShell
          }
        ) ?? null
      )
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
