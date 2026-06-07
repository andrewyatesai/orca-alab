// TS dispatch for the source-control-ai parity module: maps the shared vector
// function names to the real `src/shared/source-control-ai.ts` exports so the
// harness compares the live TS reference against the Rust port. Only the pure
// JSON-in/JSON-out normalizer is exercised here; the resolvers take a typed
// GlobalSettings slice and are covered by the in-crate Rust test port.

import { normalizeRepoSourceControlAiOverrides } from '../../../src/shared/source-control-ai'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeRepoSourceControlAiOverrides':
      return normalizeRepoSourceControlAiOverrides(input)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
