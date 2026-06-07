// TS dispatch for the agent-status-types parity module: maps the shared vector
// function names to the real `src/shared/agent-status-types.ts` exports so the
// harness compares the live TS reference against the Rust port.

import { parseAgentStatusPayload } from '../../../src/shared/agent-status-types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'parseAgentStatusPayload':
      // Single arg: the raw JSON payload string the agent sent over the hook/OSC.
      return parseAgentStatusPayload(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
