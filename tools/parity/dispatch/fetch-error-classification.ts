// TS dispatch for the fetch-error-classification parity module: maps the shared
// vector function names to the real `src/main/git/fetch-error-classification.ts`
// exports so the harness compares the live TS reference against the Rust port.

import { isMissingRemoteRefGitError } from '../../../src/main/git/fetch-error-classification'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isMissingRemoteRefGitError':
      // Vectors carry the error message string directly; the function does
      // String(error) for non-Error inputs, so a string is an exact stand-in.
      return isMissingRemoteRefGitError(input)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
