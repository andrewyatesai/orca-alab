// TS dispatch for the tui-agent-selection parity module: maps the shared vector
// function names to the real `src/shared/tui-agent-selection.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  filterEnabledTuiAgents,
  isTuiAgentEnabled,
  normalizeDisabledTuiAgents,
  pickTuiAgent
} from '../../../src/shared/tui-agent-selection'
import type { TuiAgent } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'pickTuiAgent': {
      const { preferred, detected, disabled } = input as {
        preferred: TuiAgent | 'blank' | null | undefined
        detected: TuiAgent[]
        disabled?: unknown[] | null
      }
      return pickTuiAgent(preferred, detected, disabled)
    }
    case 'normalizeDisabledTuiAgents':
      return normalizeDisabledTuiAgents(input)
    case 'isTuiAgentEnabled': {
      const { agent, disabled } = input as { agent: TuiAgent; disabled?: unknown[] | null }
      return isTuiAgentEnabled(agent, disabled)
    }
    case 'filterEnabledTuiAgents': {
      const { agents, disabled } = input as { agents: TuiAgent[]; disabled?: unknown[] | null }
      return filterEnabledTuiAgents(agents, disabled)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
