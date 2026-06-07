// TS dispatch for the mcp-env parity module: maps the shared vector function
// names to the real `src/shared/mcp-config.ts` exports so the harness compares
// the live TS reference against the Rust `orca-text::mcp_env` port.

import { maskMcpEnv } from '../../../src/shared/mcp-config'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'maskMcpEnv':
      return maskMcpEnv(input)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
