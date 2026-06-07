// TS dispatch for the mcp parity module: maps the shared vector function names
// to the real `src/shared/mcp-config.ts` exports so the harness compares the
// live TS reference against the Rust port (`orca-config::mcp`).

import {
  inspectMcpConfigContent,
  type McpConfigCandidate
} from '../../../src/shared/mcp-config'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'inspectMcpConfigContent': {
      const { candidate, content } = input as {
        candidate: McpConfigCandidate
        content: string | null
      }
      return inspectMcpConfigContent(candidate, content)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
