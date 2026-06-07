// TS dispatch for the base-ref-search-result parity module: maps the shared
// vector function names to the real `src/shared/base-ref-search-result.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  deriveLegacyLocalBranchName,
  legacyBaseRefSearchResult
} from '../../../src/shared/base-ref-search-result'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'deriveLegacyLocalBranchName':
      return deriveLegacyLocalBranchName(input as string)
    case 'legacyBaseRefSearchResult':
      return legacyBaseRefSearchResult(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
