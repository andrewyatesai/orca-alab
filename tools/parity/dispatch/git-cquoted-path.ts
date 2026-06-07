// TS dispatch for the git-cquoted-path parity module: maps the shared vector
// function names to the real `src/shared/git-cquoted-path.ts` export so the
// harness compares the live TS reference against the Rust port.

import { decodeGitCQuotedPath } from '../../../src/shared/git-cquoted-path'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'decodeGitCQuotedPath':
      return decodeGitCQuotedPath(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
