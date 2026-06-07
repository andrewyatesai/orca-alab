// TS dispatch for the agent-kind parity module: maps the shared vector
// function names to the real `src/shared/agent-kind.ts` exports so the harness
// compares the live TS reference against the Rust port.

import { agentKindToTuiAgent, tuiAgentToAgentKind } from '../../../src/shared/agent-kind'
import type { AgentKind } from '../../../src/shared/telemetry-events'
import type { TuiAgent } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'tuiAgentToAgentKind': {
      const { agent } = input as { agent: TuiAgent }
      return tuiAgentToAgentKind(agent)
    }
    case 'agentKindToTuiAgent': {
      // A missing `kind` key models the TS `undefined` arg (JSON can't encode it).
      const { kind } = input as { kind?: AgentKind | null }
      return agentKindToTuiAgent(kind)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
