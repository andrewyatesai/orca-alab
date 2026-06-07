// TS dispatch for the commit-message-models parity module: maps the shared
// vector function names to the real model-discovery parsers in
// `src/shared/commit-message-agent-spec.ts` so the harness compares the live TS
// reference against the Rust `orca-agents::commit_message_models` port.

import {
  parseCodexModels,
  parseCursorModels,
  parseLineModels,
  parsePiModels
} from '../../../src/shared/commit-message-agent-spec'

export function dispatch(fn: string, input: unknown): unknown {
  // Every parser takes a single `stdout` string argument.
  const stdout = input as string
  switch (fn) {
    case 'parseCodexModels':
      return parseCodexModels(stdout)
    case 'parseLineModels':
      return parseLineModels(stdout)
    case 'parsePiModels':
      return parsePiModels(stdout)
    case 'parseCursorModels':
      return parseCursorModels(stdout)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
