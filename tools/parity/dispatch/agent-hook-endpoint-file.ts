// TS dispatch for the agent-hook-endpoint-file parity module: maps the shared
// vector function names to the real `src/shared/agent-hook-endpoint-file.ts`
// exports so the harness compares the live TS reference against the Rust port.

import { isAgentHookEndpointFileName } from '../../../src/shared/agent-hook-endpoint-file'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isAgentHookEndpointFileName':
      return isAgentHookEndpointFileName(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
