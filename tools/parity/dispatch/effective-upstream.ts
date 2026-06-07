// TS dispatch for the effective-upstream parity module: maps the shared vector
// function names to the real `src/shared/git-effective-upstream.ts` exports so
// the harness compares the live TS reference against the Rust port.

import { splitRemoteBranchName } from '../../../src/shared/git-effective-upstream'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'splitRemoteBranchName':
      return splitRemoteBranchName(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
